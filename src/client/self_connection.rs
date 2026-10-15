use hbb_common::{bail, rendezvous_proto::ConnType, ResultType};

pub(super) fn check(
    peer: &str,
    local_id: &str,
    other_server: &str,
    conn_type: ConnType,
) -> ResultType<()> {
    if conn_type == ConnType::DEFAULT_CONN
        && other_server.is_empty()
        && !local_id.is_empty()
        && peer == local_id
    {
        bail!("Connecting to this device is not allowed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_desktop_is_rejected_without_retry() {
        let error = check("123456789", "123456789", "", ConnType::DEFAULT_CONN)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "Connecting to this device is not allowed");
        assert!(!super::super::check_if_retry(
            "error",
            "Connection Error",
            &error,
            false
        ));
    }

    #[test]
    fn other_devices_servers_and_connection_types_are_preserved() {
        assert!(check("987654321", "123456789", "", ConnType::DEFAULT_CONN).is_ok());
        assert!(check("123456789", "", "", ConnType::DEFAULT_CONN).is_ok());
        assert!(check(
            "123456789",
            "123456789",
            "other.example",
            ConnType::DEFAULT_CONN
        )
        .is_ok());
        for conn_type in [
            ConnType::FILE_TRANSFER,
            ConnType::PORT_FORWARD,
            ConnType::RDP,
            ConnType::VIEW_CAMERA,
            ConnType::TERMINAL,
        ] {
            assert!(check("123456789", "123456789", "", conn_type).is_ok());
        }
    }
}
