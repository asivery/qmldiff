#[macro_export]
macro_rules! error_received_expected {
    ($recvd: expr, $expected: expr) => {
        Err(Error::msg(format!(
            "Error while parsing: expected {}, got {:?}",
            $expected, $recvd
        )))
    };
}
