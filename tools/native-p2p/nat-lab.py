#!/usr/bin/env python3
"""Isolated Linux NAT acceptance. Never changes host routes or firewall tables.

Each trial creates five uniquely named network namespaces, real conntrack/SNAT,
an isolated hbbs and four STUN destinations. The diagnostic links the production
UDP module and includes RustDesk's KcpStream source. Only lab-owned resources are
removed. Run as root; binaries are explicit inputs, never downloaded by this tool.
"""
import argparse
import hashlib
import traceback
import json
import os
from pathlib import Path
import selectors
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import time
import uuid


def run(args, **kwargs):
    result = subprocess.run(args, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, **kwargs)
    if result.returncode:
        raise RuntimeError(f'{args}: {result.stderr.strip()}')
    return result


def stun_server():
    selector = selectors.DefaultSelector()
    addresses = [('192.0.2.100', 3478), ('192.0.2.101', 3479),
                 ('192.0.2.100', 3479), ('192.0.2.101', 3478)]
    for address in addresses:
        udp = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        udp.bind(address)
        selector.register(udp, selectors.EVENT_READ)
    print('ready', flush=True)
    while True:
        for key, _ in selector.select():
            data, peer = key.fileobj.recvfrom(1500)
            if len(data) != 20 or data[:2] != b'\0\1' or data[4:8] != b'\x21\x12\xa4\x42':
                continue
            alternate = addresses[1] if key.fileobj.getsockname() == addresses[0] else addresses[0]
            def attribute(kind, address):
                return struct.pack('!HHBBH4s', kind, 8, 0, 1, address[1], socket.inet_aton(address[0]))
            attributes = attribute(1, peer) + attribute(0x802c, alternate)
            key.fileobj.sendto(struct.pack('!HH', 0x101, len(attributes)) + data[4:20] + attributes, peer)


class Lab:
    def __init__(self, directory):
        self.prefix = 'np-' + uuid.uuid4().hex[:8]
        self.names = {name: self.prefix + '-' + name for name in ['wan', 'a', 'b', 'ga', 'gb']}
        self.created = []
        self.processes = []
        self.files = []
        self.directory = directory

    def ip(self, name, *args):
        return run(['ip', '-n', self.names[name], *args])

    def command(self, name, *args):
        return ['ip', 'netns', 'exec', self.names[name], *map(str, args)]

    def spawn(self, name, args, output, cwd=None, env=None):
        handle = (self.directory / output).open('w')
        self.files.append(handle)
        process = subprocess.Popen(self.command(name, *args), cwd=cwd, env=env,
                                   stdout=handle, stderr=subprocess.STDOUT, start_new_session=True)
        self.processes.append(process)
        return process

    def pair(self, left, left_device, right, right_device):
        # Create both ends inside a lab namespace, so no host interface is touched.
        self.ip(left, 'link', 'add', left_device, 'type', 'veth', 'peer', 'name', 'tmp-peer')
        self.ip(left, 'link', 'set', 'tmp-peer', 'netns', self.names[right])
        self.ip(right, 'link', 'set', 'tmp-peer', 'name', right_device)
        self.ip(left, 'link', 'set', 'dev', left_device, 'up')
        self.ip(right, 'link', 'set', 'dev', right_device, 'up')

    def setup(self, kinds, loss=False):
        for name, namespace in self.names.items():
            run(['ip', 'netns', 'add', namespace])
            self.created.append(namespace)
            self.ip(name, 'link', 'set', 'lo', 'up')
        self.ip('wan', 'link', 'add', 'br0', 'type', 'bridge')
        self.ip('wan', 'link', 'set', 'br0', 'up')
        for ip in ['192.0.2.100', '192.0.2.101']:
            self.ip('wan', 'addr', 'add', ip + '/24', 'dev', 'br0')
        for index, name in enumerate(['a', 'b'], 1):
            gateway = 'g' + name
            public = '192.0.2.' + str(index * 10)
            private = f'172.20.{index}'
            self.pair(name, 'eth0', gateway, 'lan')
            self.pair(gateway, 'wan', 'wan', name)
            self.ip('wan', 'link', 'set', 'dev', name, 'master', 'br0')
            self.ip(name, 'addr', 'add', private + '.2/24', 'dev', 'eth0')
            self.ip(gateway, 'addr', 'add', private + '.1/24', 'dev', 'lan')
            self.ip(gateway, 'addr', 'add', public + '/24', 'dev', 'wan')
            self.ip(name, 'route', 'add', 'default', 'via', private + '.1')
            run(self.command(gateway, 'sysctl', '-qw', 'net.ipv4.ip_forward=1'))
            run(self.command(gateway, 'sysctl', '-qw', 'net.ipv4.conf.all.rp_filter=0'))
            kind = kinds[index - 1]
            if kind in ['multi', 'unverified']:
                self.ip(gateway, 'addr', 'add', '192.0.2.11/24', 'dev', 'wan')
            self.nat(gateway, private + '.2', public, kind, 40000 + index * 1000)
            if loss:
                run(self.command(gateway, 'tc', 'qdisc', 'add', 'dev', 'wan', 'root', 'netem', 'delay', '12ms', '4ms', 'loss', '1%'))

    def lan(self):
        self.pair('a', 'lan0', 'b', 'lan0')
        for index, side in enumerate(['a', 'b'], 1):
            self.ip(side, 'addr', 'add', f'192.168.50.{index}/24', 'dev', 'lan0')

    def public_client(self):
        self.ip('a', 'addr', 'add', '192.0.3.2/24', 'dev', 'eth0')
        self.ip('a', 'addr', 'delete', '172.20.1.2/24', 'dev', 'eth0')
        self.ip('ga', 'addr', 'add', '192.0.3.1/24', 'dev', 'lan')
        self.ip('a', 'route', 'replace', 'default', 'via', '192.0.3.1')
        self.ip('wan', 'route', 'add', '192.0.3.0/24', 'via', '192.0.2.10')
        self.ip('gb', 'route', 'add', '192.0.3.0/24', 'via', '192.0.2.10')

    def nat(self, gateway, private, public, kind, port):
        before = ''
        tcp = f'meta l4proto tcp counter snat to {public}'
        rules = []
        if kind == 'easy':
            before = f'iifname "wan" ip saddr @opened meta l4proto udp counter dnat to {private}'
            rules = [f'meta l4proto udp add @opened {{ ip daddr timeout 60s }} counter snat to {public}']
        elif kind == 'predict':
            for offset, (ip, dst) in enumerate([('192.0.2.100', 3478), ('192.0.2.101', 3479), ('192.0.2.100', 3479), ('192.0.2.101', 3478)]):
                rules.append(f'ip daddr {ip} udp dport {dst} counter snat to {public}:{port + offset}')
            # Address-dependent mapping with endpoint-dependent filtering. A new peer IP
            # takes the next port; destination ports on that peer retain the mapping.
            rules.append(f'meta l4proto udp counter snat to {public}:{port + 4}')
        elif kind in ['multi', 'unverified']:
            if kind == 'multi':
                tcp = 'meta l4proto tcp counter snat to numgen inc mod 2 map { 0 : 192.0.2.10, 1 : 192.0.2.11 }'
                rules.append(f'ip daddr 192.0.2.100 udp dport 3478 counter snat to {public}:1024-65534 fully-random')
            rules.append('meta l4proto udp counter snat to 192.0.2.11:1024-65534 fully-random')
        else:
            rules = [f'meta l4proto udp counter snat to {public}:1024-65534 fully-random']
        drop = 'udp dport { 3478, 3479 } counter drop' if kind == 'blocked' else ''
        if kind == 'blackhole':
            drop = 'iifname "lan" meta l4proto udp ip daddr != { 192.0.2.100, 192.0.2.101 } counter drop'
        ruleset = f'''table ip native_lab {{
 set opened {{ type ipv4_addr; flags dynamic,timeout; timeout 60s; }}
 chain input {{ type filter hook input priority filter; policy accept; iifname "wan" meta l4proto udp counter drop; }}
 chain forward {{ type filter hook forward priority filter; policy accept; {drop}; }}
 chain prerouting {{ type nat hook prerouting priority dstnat; policy accept; {before}; }}
 chain postrouting {{ type nat hook postrouting priority srcnat; policy accept;
   oifname "wan" ip saddr {private} jump outbound
 }}
 chain outbound {{ {tcp}; {'; '.join(rules)}; }}
}}
'''
        (self.directory / (gateway + '.nft')).write_text(ruleset)
        run(self.command(gateway, 'nft', '-f', '-'), input=ruleset)

    def close(self):
        for process in reversed(self.processes):
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=4)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=4)
        for file in self.files:
            file.close()
        for name in reversed(self.created):
            assert name.startswith(self.prefix + '-') and name in self.names.values()
            run(['ip', 'netns', 'delete', name])


def records(path):
    values = []
    if not path.exists():
        return values
    for line in path.read_text(errors='replace').splitlines():
        try:
            values.append(json.loads(line))
        except ValueError:
            pass
    return values


def trial(args, scenario, number, kinds, expected):
    directory = args.output / f'{scenario}-{number:02d}'
    directory.mkdir(parents=True, exist_ok=False)
    lab = Lab(directory)
    result = {'scenario': scenario, 'trial': number, 'expected_native': expected, 'started': time.time(), 'binaries': args.hashes, 'nat': kinds}
    private = tempfile.TemporaryDirectory(prefix=lab.prefix + '-')
    try:
        lab.setup(kinds, loss=scenario == 'loss')
        if scenario == 'lan':
            lab.lan()
        if scenario == 'public':
            lab.public_client()
        server = lab.spawn('wan', [args.hbbs, '-p', '22116'], 'hbbs.log', cwd=private.name)
        stun = lab.spawn('wan', [sys.executable, Path(__file__).resolve(), '--stun'], 'stun.log')
        key = Path(private.name) / 'id_ed25519.pub'
        for _ in range(80):
            if server.poll() is not None or stun.poll() is not None:
                raise RuntimeError('lab server failed to start')
            if key.exists() and 'Listening on tcp/udp' in (directory / 'hbbs.log').read_text(errors='replace') and 'ready' in (directory / 'stun.log').read_text():
                break
            time.sleep(.1)
        else:
            raise RuntimeError('lab listener readiness timeout')
        common = [args.probe, 'listen', '192.0.2.100:22116', key.read_text().strip()]
        env = dict(os.environ, NATIVE_P2P_STUN='192.0.2.100:3478', TOKIO_WORKER_THREADS='2')
        listener_env = dict(env)
        if scenario == 'lan':
            listener_env['NATIVE_P2P_LAB_INTERFACE'] = '192.168.50.2/24'
        listener = lab.spawn('b', common, 'listener.jsonl', env=listener_env)
        target = None
        for _ in range(100):
            target = next((r['data']['id'] for r in records(directory / 'listener.jsonl') if r.get('event') == 'ready'), None)
            if target or listener.poll() is not None:
                break
            time.sleep(.1)
        if not target:
            raise RuntimeError('peer listener did not register')
        if scenario == 'lan':
            env['NATIVE_P2P_LAB_INTERFACE'] = '192.168.50.1/24'
        if scenario == 'public':
            env['NATIVE_P2P_LAB_INTERFACE'] = '192.0.3.2/24'
        if scenario == 'cancel':
            env['NATIVE_P2P_CANCEL_MS'] = '1200'
        if scenario == 'bad-proof':
            env['NATIVE_P2P_BAD_PROOF'] = '1'
        caller = lab.spawn('a', [args.probe, 'connect', common[2], common[3], target], 'caller.jsonl', env=env)
        result['caller_exit'] = caller.wait(timeout=52)
        result['listener_exit'] = listener.wait(timeout=52)
        for side in ['caller', 'listener']:
            result[side] = records(directory / (side + '.jsonl'))
        result['native'] = all(any(r.get('event') == 'verified' and r['data'].get('transport') == 'authenticated-kcp-diagnostic' for r in result[side]) for side in ['caller', 'listener'])
        result['sockets_after'] = {side: run(lab.command(side, 'ss', '-uanH')).stdout.strip() for side in ['a', 'b']}
        for side in ['ga', 'gb']:
            (directory / (side + '-counters.json')).write_text(run(lab.command(side, 'nft', '-j', 'list', 'ruleset')).stdout)
        result['passed'] = result['native'] == expected and not any(result['sockets_after'].values())
    except Exception as error:
        result.update(passed=False, error=str(error), traceback=traceback.format_exc())
    finally:
        lab.close()
        private.cleanup()
        result['namespaces_removed'] = all(name not in run(['ip', 'netns', 'list']).stdout for name in lab.created)
        result['finished'] = time.time()
        (directory / 'result.json').write_text(json.dumps(result, indent=2))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--hbbs', type=Path)
    parser.add_argument('--probe', type=Path)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--rounds', type=int, default=10)
    parser.add_argument('--scenarios', default='direct,predict,birthday,both-predict,mixed,multi,loss,blocked,unverified')
    parser.add_argument('--stun', action='store_true')
    args = parser.parse_args()
    if args.stun:
        stun_server()
        return
    assert os.geteuid() == 0, 'network namespaces require root'
    assert args.hbbs and args.hbbs.is_file() and args.probe and args.probe.is_file()
    assert args.output and 1 <= args.rounds <= 10
    args.hashes = {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in [args.hbbs, args.probe]}
    args.output.mkdir(parents=True, exist_ok=True)
    cases = {'direct': ('easy', 'easy', True), 'predict': ('easy', 'predict', True),
             'birthday': ('easy', 'random', True), 'both-predict': ('predict', 'predict', True),
             'mixed': ('predict', 'random', True), 'multi': ('multi', 'easy', True),
             'loss': ('easy', 'random', True), 'blocked': ('blocked', 'easy', False),
             'unverified': ('unverified', 'easy', False), 'both-random': ('random', 'random', False),
             'lan': ('random', 'random', True), 'public': ('random', 'random', True),
             'cancel': ('easy', 'random', False), 'bad-proof': ('easy', 'easy', False)}
    cases['blackhole'] = ('easy', 'blackhole', False)
    results = []
    for name in args.scenarios.split(','):
        a, b, expected = cases[name]
        for number in range(1, args.rounds + 1):
            result = trial(args, name, number, (a, b), expected)
            results.append(result)
            print(json.dumps({k: result.get(k) for k in ['scenario', 'trial', 'native', 'passed', 'error', 'namespaces_removed']}), flush=True)
            (args.output / 'results.json').write_text(json.dumps(results, indent=2))
    failures = []
    for name in args.scenarios.split(','):
        group = [r for r in results if r['scenario'] == name]
        threshold = 9 if args.rounds == 10 and cases[name][2] else args.rounds
        if sum(bool(r.get('passed')) for r in group) < threshold:
            failures.append(name)
    print(json.dumps({'failed_scenarios': failures}), flush=True)
    sys.exit(bool(failures))


if __name__ == '__main__':
    main()
