//! Registry logins from a cloud's own credentials, as E2B's
//! `from_aws_registry` and `from_gcp_registry` send them: exchanged here
//! for the username and password the registry takes.
//!
//! - AWS: `ecr:GetAuthorizationToken`, signed with Signature Version 4,
//!   answers a token that is `AWS:<password>`, base64.
//! - Google: the service account's key signs a JWT (RS256), which Google's
//!   token endpoint exchanges for an access token; Artifact Registry and
//!   Container Registry take it as the password of `oauth2accesstoken`.
//!
//! Neither credential is kept or logged: each is used for the one pull.

use serde_json::Value;
use sha2::Digest;

use crate::oci::Credentials;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key);
    ring::hmac::sign(&key, data).as_ref().to_vec()
}

/// `YYYYMMDDTHHMMSSZ` for a Unix time.
fn amz_date(unix: u64) -> String {
    let days = i64::try_from(unix / 86_400).unwrap_or(0);
    let secs = unix % 86_400;
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

/// A request signed with AWS Signature Version 4: the `Authorization`
/// header for it. `headers` are the ones signed, `host` and `x-amz-date`
/// among them, lowercase.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sigv4(
    method: &str,
    path: &str,
    query: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    key_id: &str,
    secret: &str,
    region: &str,
    service: &str,
    amz_date: &str,
) -> String {
    let mut headers: Vec<(String, String)> = headers
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    headers.sort();
    let canonical_headers: String = headers.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let signed: Vec<&str> = headers.iter().map(|(k, _)| k.as_str()).collect();
    let signed = signed.join(";");
    let canonical = format!(
        "{method}\n{path}\n{query}\n{canonical_headers}\n{signed}\n{}",
        hex(&sha2::Sha256::digest(body))
    );
    let date = &amz_date[..8];
    let scope = format!("{date}/{region}/{service}/aws4_request");
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex(&sha2::Sha256::digest(canonical.as_bytes()))
    );
    let key = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let key = hmac(&key, region.as_bytes());
    let key = hmac(&key, service.as_bytes());
    let key = hmac(&key, b"aws4_request");
    let signature = hex(&hmac(&key, to_sign.as_bytes()));
    format!("AWS4-HMAC-SHA256 Credential={key_id}/{scope}, SignedHeaders={signed}, Signature={signature}")
}

/// ECR's registry login for these credentials.
pub(crate) async fn ecr(
    http: &reqwest::Client,
    key_id: &str,
    secret: &str,
    region: &str,
) -> Result<Credentials, String> {
    if key_id.is_empty() || secret.is_empty() || region.is_empty() {
        return Err(
            "an AWS registry login needs awsAccessKeyId, awsSecretAccessKey and awsRegion".into(),
        );
    }
    if !region
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(format!("awsRegion {region:?}"));
    }
    // An ECR-compatible endpoint -- FIPS, or a VPC endpoint -- instead of
    // the region's public one.
    let endpoint = std::env::var("HV2_ECR_ENDPOINT")
        .unwrap_or_else(|_| format!("https://api.ecr.{region}.amazonaws.com"));
    let url = reqwest::Url::parse(&endpoint).map_err(|e| format!("{endpoint}: {e}"))?;
    let host = match (url.host_str(), url.port()) {
        (Some(h), Some(p)) => format!("{h}:{p}"),
        (Some(h), None) => h.to_string(),
        _ => return Err(format!("{endpoint}: no host")),
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let date = amz_date(now);
    let body = b"{}";
    let target = "AmazonEC2ContainerRegistry_V20150921.GetAuthorizationToken";
    let content_type = "application/x-amz-json-1.1";
    let authorization = sigv4(
        "POST",
        "/",
        "",
        &[
            ("content-type", content_type),
            ("host", &host),
            ("x-amz-date", &date),
            ("x-amz-target", target),
        ],
        body,
        key_id,
        secret,
        region,
        "ecr",
        &date,
    );
    let response = http
        .post(url)
        .header("content-type", content_type)
        .header("x-amz-date", &date)
        .header("x-amz-target", target)
        .header("authorization", authorization)
        .body(&body[..])
        .send()
        .await
        .map_err(|e| format!("ECR: {e}"))?;
    let status = response.status();
    let answer: Value = response
        .json()
        .await
        .map_err(|e| format!("ECR answered {status}: {e}"))?;
    if !status.is_success() {
        return Err(format!(
            "ECR refused the login ({status}): {}",
            answer["message"]
                .as_str()
                .or_else(|| answer["Message"].as_str())
                .unwrap_or("no reason given")
        ));
    }
    let token = answer["authorizationData"][0]["authorizationToken"]
        .as_str()
        .ok_or("ECR answered without an authorization token")?;
    let decoded = hv2_guest_agent::b64::decode(token).ok_or("ECR's token is not base64")?;
    let decoded = String::from_utf8(decoded).map_err(|_| "ECR's token is not text")?;
    let (username, password) = decoded
        .split_once(':')
        .ok_or("ECR's token is not user:password")?;
    Ok(Credentials {
        username: username.to_string(),
        password: password.to_string(),
    })
}

/// The DER inside a PEM block.
fn pem_der(pem: &str) -> Result<Vec<u8>, String> {
    let body: String = pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect::<String>()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    hv2_guest_agent::b64::decode(&body).ok_or_else(|| "the private key is not PEM".to_string())
}

/// A service account's signed assertion, for `aud`, issued at `now`.
pub(crate) fn gcp_assertion(key: &Value, now: u64) -> Result<String, String> {
    let email = key["client_email"]
        .as_str()
        .ok_or("serviceAccountJson has no client_email")?;
    let pem = key["private_key"]
        .as_str()
        .ok_or("serviceAccountJson has no private_key")?;
    let aud = key["token_uri"]
        .as_str()
        .unwrap_or("https://oauth2.googleapis.com/token");
    let header = serde_json::json!({ "alg": "RS256", "typ": "JWT", "kid": key["private_key_id"] });
    let claims = serde_json::json!({
        "iss": email,
        "scope": "https://www.googleapis.com/auth/cloud-platform",
        "aud": aud,
        "iat": now,
        "exp": now + 3600,
    });
    let signing_input = format!(
        "{}.{}",
        crate::identity::b64url(header.to_string().as_bytes()),
        crate::identity::b64url(claims.to_string().as_bytes())
    );
    let pair = ring::signature::RsaKeyPair::from_pkcs8(&pem_der(pem)?)
        .map_err(|e| format!("the service account's private key: {e}"))?;
    let mut signature = vec![0u8; pair.public().modulus_len()];
    pair.sign(
        &ring::signature::RSA_PKCS1_SHA256,
        &ring::rand::SystemRandom::new(),
        signing_input.as_bytes(),
        &mut signature,
    )
    .map_err(|e| format!("signing the assertion: {e}"))?;
    Ok(format!(
        "{signing_input}.{}",
        crate::identity::b64url(&signature)
    ))
}

/// Google's registry login for a service account's JSON key.
pub(crate) async fn gcp(
    http: &reqwest::Client,
    service_account_json: &str,
) -> Result<Credentials, String> {
    let key: Value = serde_json::from_str(service_account_json)
        .map_err(|e| format!("serviceAccountJson: {e}"))?;
    let token_uri = key["token_uri"]
        .as_str()
        .unwrap_or("https://oauth2.googleapis.com/token")
        .to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let assertion = gcp_assertion(&key, now)?;
    let form = form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer")
        .append_pair("assertion", &assertion)
        .finish();
    let response = http
        .post(&token_uri)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(form)
        .send()
        .await
        .map_err(|e| format!("{token_uri}: {e}"))?;
    let status = response.status();
    let answer: Value = response
        .json()
        .await
        .map_err(|e| format!("{token_uri} answered {status}: {e}"))?;
    let token = answer["access_token"].as_str().ok_or_else(|| {
        format!(
            "Google refused the service account ({status}): {}",
            answer["error_description"]
                .as_str()
                .or_else(|| answer["error"].as_str())
                .unwrap_or("no reason given")
        )
    })?;
    Ok(Credentials {
        username: "oauth2accesstoken".into(),
        password: token.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AWS's own worked example of Signature Version 4 (IAM `ListUsers`).
    #[test]
    fn sigv4_matches_awss_example() {
        let authorization = sigv4(
            "GET",
            "/",
            "Action=ListUsers&Version=2010-05-08",
            &[
                (
                    "content-type",
                    "application/x-www-form-urlencoded; charset=utf-8",
                ),
                ("host", "iam.amazonaws.com"),
                ("x-amz-date", "20150830T123600Z"),
            ],
            b"",
            "AKIDEXAMPLE",
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "us-east-1",
            "iam",
            "20150830T123600Z",
        );
        assert_eq!(
            authorization,
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/iam/aws4_request, \
             SignedHeaders=content-type;host;x-amz-date, \
             Signature=5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7"
        );
    }

    /// The assertion Google verifies: RS256 over header and claims, signed
    /// by the service account's key. The key is a throwaway made for this
    /// test, and signs nothing else.
    #[test]
    fn gcp_assertions_are_signed_jwts() {
        let pem = include_str!("../testdata/test-only-rsa-key.pem");
        let key = serde_json::json!({
            "client_email": "builder@project.iam.gserviceaccount.com",
            "private_key": pem,
            "private_key_id": "k1",
            "token_uri": "https://oauth2.googleapis.com/token",
        });
        let jwt = gcp_assertion(&key, 1_700_000_000).unwrap();
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3);
        let claims: Value = serde_json::from_slice(&b64url_decode(parts[1])).unwrap();
        assert_eq!(claims["iss"], "builder@project.iam.gserviceaccount.com");
        assert_eq!(claims["aud"], "https://oauth2.googleapis.com/token");
        assert_eq!(claims["exp"], 1_700_003_600);
        let header: Value = serde_json::from_slice(&b64url_decode(parts[0])).unwrap();
        assert_eq!(header["alg"], "RS256");

        let pair = ring::signature::RsaKeyPair::from_pkcs8(&pem_der(pem).unwrap()).unwrap();
        let public = ring::signature::UnparsedPublicKey::new(
            &ring::signature::RSA_PKCS1_2048_8192_SHA256,
            ring::signature::KeyPair::public_key(&pair)
                .as_ref()
                .to_vec(),
        );
        let signed = format!("{}.{}", parts[0], parts[1]);
        assert!(public
            .verify(signed.as_bytes(), &b64url_decode(parts[2]))
            .is_ok());
        assert!(public
            .verify(b"something else", &b64url_decode(parts[2]))
            .is_err());
    }

    fn b64url_decode(s: &str) -> Vec<u8> {
        let standard: String = s
            .chars()
            .map(|c| match c {
                '-' => '+',
                '_' => '/',
                c => c,
            })
            .collect();
        let padded = format!("{standard}{}", "=".repeat((4 - standard.len() % 4) % 4));
        hv2_guest_agent::b64::decode(&padded).unwrap()
    }

    #[test]
    fn dates_are_amzs() {
        assert_eq!(amz_date(1_440_938_160), "20150830T123600Z");
        assert_eq!(amz_date(0), "19700101T000000Z");
        assert_eq!(amz_date(951_782_400), "20000229T000000Z");
    }
}
