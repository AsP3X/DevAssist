//! HTTP GET under the program's name, with an optional sign-in.

use std::sync::OnceLock;
use std::time::Duration;

/// What a GET answers with, whatever its status.
pub type Response = ureq::http::Response<ureq::Body>;

/// How patient a request is.
#[derive(Clone, Copy)]
pub enum Profile {
    /// Give up after 20 seconds in total. For small API answers.
    Quick,
    /// May take long as a whole, but must keep moving. For downloads.
    Steady,
}

fn agent(profile: Profile) -> &'static ureq::Agent {
    #[cfg(feature = "testing")]
    super::note(super::Reach::Network);
    static QUICK: OnceLock<ureq::Agent> = OnceLock::new();
    static STEADY: OnceLock<ureq::Agent> = OnceLock::new();
    match profile {
        Profile::Quick => QUICK.get_or_init(|| {
            ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(20)))
                // The caller judges the status: the body often says why.
                .http_status_as_error(false)
                .build()
                .into()
        }),
        Profile::Steady => STEADY.get_or_init(|| {
            ureq::Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(15)))
                .timeout_recv_response(Some(Duration::from_secs(30)))
                .timeout_per_read(Some(Duration::from_secs(30)))
                .http_status_as_error(false)
                .build()
                .into()
        }),
    }
}

/// Asks for `url`. An answer with any status comes back; an error means the
/// server could not be reached. `bearer` is sent as the sign-in.
pub fn get(
    url: &str,
    profile: Profile,
    headers: &[(&str, &str)],
    bearer: Option<&str>,
) -> Result<Response, String> {
    let mut request = agent(profile).get(url).header("User-Agent", "devassist");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    if let Some(token) = bearer {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    request.call().map_err(|err| err.to_string())
}

/// Sends `body` as JSON to `url`. Like `get`, an answer with any status
/// comes back, and `bearer` is sent as the sign-in.
pub fn post_json(
    url: &str,
    profile: Profile,
    headers: &[(&str, &str)],
    bearer: Option<&str>,
    body: &str,
) -> Result<Response, String> {
    let mut request = agent(profile)
        .post(url)
        .header("User-Agent", "devassist")
        .header("Content-Type", "application/json");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    if let Some(token) = bearer {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    request.send(body).map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread::{self, JoinHandle};

    /// Answers one request with `reply` and hands back what was asked,
    /// in lowercase.
    fn serve_once(reply: &'static str) -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/thing", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut seen = Vec::new();
            let mut chunk = [0u8; 1024];
            while !seen.windows(4).any(|window| window == b"\r\n\r\n") {
                let size = stream.read(&mut chunk).unwrap();
                if size == 0 {
                    break;
                }
                seen.extend_from_slice(&chunk[..size]);
            }
            // A request with a body is read to its end before the answer.
            let text = String::from_utf8_lossy(&seen).to_lowercase();
            let head = text.find("\r\n\r\n").map_or(seen.len(), |at| at + 4);
            let body: usize = text
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .and_then(|length| length.trim().parse().ok())
                .unwrap_or(0);
            while seen.len() < head + body {
                let size = stream.read(&mut chunk).unwrap();
                if size == 0 {
                    break;
                }
                seen.extend_from_slice(&chunk[..size]);
            }
            stream.write_all(reply.as_bytes()).unwrap();
            String::from_utf8_lossy(&seen).to_lowercase()
        });
        (url, server)
    }

    #[test]
    fn get_sends_the_name_the_headers_and_the_sign_in() {
        let (url, server) =
            serve_once("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        let mut response = get(
            &url,
            Profile::Quick,
            &[("Accept", "text/plain")],
            Some("secret"),
        )
        .unwrap();
        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(response.body_mut().read_to_string().unwrap(), "ok");
        let asked = server.join().unwrap();
        assert!(asked.contains("user-agent: devassist"));
        assert!(asked.contains("accept: text/plain"));
        assert!(asked.contains("authorization: bearer secret"));
    }

    #[test]
    fn post_sends_the_body_as_json() {
        let (url, server) =
            serve_once("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        let mut response =
            post_json(&url, Profile::Quick, &[], Some("secret"), "{\"a\":1}").unwrap();
        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(response.body_mut().read_to_string().unwrap(), "ok");
        let asked = server.join().unwrap();
        assert!(asked.starts_with("post /thing"), "{asked}");
        assert!(asked.contains("content-type: application/json"), "{asked}");
        assert!(asked.contains("authorization: bearer secret"), "{asked}");
        assert!(asked.ends_with("{\"a\":1}"), "{asked}");
    }

    #[test]
    fn get_without_a_sign_in_sends_no_authorization() {
        let (url, server) =
            serve_once("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        get(&url, Profile::Steady, &[], None).unwrap();
        assert!(!server.join().unwrap().contains("authorization"));
    }

    #[test]
    fn a_refusal_comes_back_as_an_answer_not_an_error() {
        let (url, server) = serve_once(
            "HTTP/1.1 404 Not Found\r\nContent-Length: 4\r\nConnection: close\r\n\r\nnope",
        );
        let response = get(&url, Profile::Quick, &[], None).unwrap();
        assert_eq!(response.status().as_u16(), 404);
        server.join().unwrap();
    }
}
