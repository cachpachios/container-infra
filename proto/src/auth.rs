use hmac::Hmac;
use hmac::Mac;
use jwt::Claims;
use jwt::RegisteredClaims;
use jwt::SignWithKey;
use jwt::VerifyWithKey;
use sha2::Sha256;

pub fn validate_authentication(
    token: &str,
    secret: &Hmac<Sha256>,
    expected_audience: Option<&str>,
) -> bool {
    let claims: Claims = match token.verify_with_key(secret) {
        Ok(claims) => claims,
        Err(_) => return false,
    };

    if expected_audience != claims.registered.audience.as_deref() {
        return false;
    }

    let unix_t_s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    unix_t_s < claims.registered.expiration.unwrap_or(0)
        && unix_t_s > claims.registered.not_before.unwrap_or(0)
}

pub fn validate_authentication_secrets_as_bytes(
    token: &str,
    secret: &[u8],
    expected_audience: Option<&str>,
) -> bool {
    let secret = Hmac::<Sha256>::new_from_slice(secret).unwrap();
    validate_authentication(token, &secret, expected_audience)
}

pub fn sign_token(secret: &Hmac<Sha256>, audience: Option<String>) -> String {
    let unix_t_s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let claims = Claims::new(RegisteredClaims {
        issuer: None,
        audience: audience,
        expiration: Some(unix_t_s + 60 * 60),
        subject: None,
        not_before: Some(unix_t_s - 60),
        issued_at: None,
        json_web_token_id: None,
    });

    claims.sign_with_key(secret).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    #[test]
    fn test_sign_token() {
        let token = sign_token(&Hmac::<Sha256>::new_from_slice(b"secret").unwrap(), None);
        assert!(!token.is_empty());
        assert!(token.split(".").count() == 3);
        assert!(token.starts_with("eyJ"));
    }

    #[test]
    fn test_validate_authentication() {
        let secret = Hmac::<Sha256>::new_from_slice(b"secret").unwrap();
        let token = sign_token(&secret, Some("my_aud".to_string()));

        assert!(validate_authentication(&token, &secret, Some("my_aud")));
    }

    #[test]
    fn test_validate_authentication_invalid() {
        let secret = Hmac::<Sha256>::new_from_slice(b"secret").unwrap();
        let token = sign_token(&secret, None);
        assert!(!validate_authentication(&token, &secret, Some("some_aud")));
        assert!(!validate_authentication_secrets_as_bytes(
            &token,
            b"wrong_secret",
            Some("some_aud"),
        ));
    }
}
