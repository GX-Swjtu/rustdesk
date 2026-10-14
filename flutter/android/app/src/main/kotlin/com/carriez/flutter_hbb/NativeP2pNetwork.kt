package com.carriez.flutter_hbb

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.util.Log
import org.json.JSONArray
import org.json.JSONObject
import java.net.Inet4Address

/** Optional LAN candidates, queried only for a new native P2P attempt. */
object NativeP2pNetwork {
    @Suppress("DEPRECATION")
    fun interfaces(context: Context): String = try {
        val cm = context.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        val addresses = cm.allNetworks.flatMap { network ->
            val caps = cm.getNetworkCapabilities(network)
            if (caps == null || !caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN)
                || caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN)
                || !(caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI)
                    || caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET))
                || cm.getNetworkInfo(network)?.isConnected != true) emptyList()
            else cm.getLinkProperties(network)?.linkAddresses ?: emptyList()
        }.filter {
            val ip = it.address
            ip is Inet4Address && !ip.isAnyLocalAddress && !ip.isLoopbackAddress
                && !ip.isLinkLocalAddress && !ip.isMulticastAddress && it.prefixLength in 1..32
        }.distinctBy { it.toString() }.sortedBy { it.toString() }.take(8)
        JSONArray().apply {
            addresses.forEach { put(JSONObject().put("address", it.address.hostAddress).put("prefix", it.prefixLength)) }
        }.toString()
    } catch (error: Exception) {
        Log.w("NativeP2pNetwork", "LAN candidates unavailable", error)
        "[]"
    }
}
