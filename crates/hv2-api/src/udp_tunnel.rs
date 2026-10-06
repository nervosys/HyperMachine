//! HTTP/1.1 upgrade carrying bounded length-framed UDP datagrams.
//! Authentication, guest selection and frame validation belong to the caller
//! and guest relay. This layer preserves the framed byte stream unchanged.
use axum::extract::Request;
use axum::response::Response;
use tokio::io::{AsyncRead, AsyncWrite};

pub const PROTOCOL: &str = "hv2-udp/1";

pub fn validate(request: &Request) -> Result<(), &'static str> {
    crate::tcp_tunnel::validate_protocol(request, PROTOCOL)
}

pub fn accept<T>(request: Request, backend: T) -> Response
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    crate::tcp_tunnel::accept_protocol(request, backend, PROTOCOL)
}

/// Explicit IPv6 destination selection; its frames retain the 65,507-byte bound.
pub const PROTOCOL_IPV6: &str = "hv2-udp6/1";

pub fn validate_ipv6(request: &Request) -> Result<(), &'static str> {
    crate::tcp_tunnel::validate_protocol(request, PROTOCOL_IPV6)
}

pub fn accept_ipv6<T>(request: Request, backend: T) -> Response
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    crate::tcp_tunnel::accept_protocol(request, backend, PROTOCOL_IPV6)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Method, Version};

    #[test]
    fn udp_address_families_require_their_exact_protocol() {
        for protocol in [PROTOCOL, PROTOCOL_IPV6] {
            let request = Request::builder()
                .header("connection", "upgrade")
                .header("upgrade", protocol)
                .body(Body::empty())
                .unwrap();
            assert_eq!(validate(&request).is_ok(), protocol == PROTOCOL);
            assert_eq!(validate_ipv6(&request).is_ok(), protocol == PROTOCOL_IPV6);
        }
    }

    #[test]
    fn udp_negotiation_refuses_tcp_body_and_wrong_http_version() {
        for (protocol, method, version, length, valid) in [
            (PROTOCOL, Method::GET, Version::HTTP_11, "0", true),
            (
                crate::tcp_tunnel::PROTOCOL,
                Method::GET,
                Version::HTTP_11,
                "0",
                false,
            ),
            (PROTOCOL, Method::POST, Version::HTTP_11, "0", false),
            (PROTOCOL, Method::GET, Version::HTTP_2, "0", false),
            (PROTOCOL, Method::GET, Version::HTTP_11, "1", false),
        ] {
            let request = Request::builder()
                .method(method)
                .version(version)
                .header("connection", "upgrade")
                .header("upgrade", protocol)
                .header("content-length", length)
                .body(Body::empty())
                .unwrap();
            assert_eq!(validate(&request).is_ok(), valid);
        }
    }
}
