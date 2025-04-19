use std::{char::from_u32, time::SystemTime};

use serde::de::DeserializeOwned;

const MMDS_IP_ADDR: &str = "169.254.169.254";

pub struct MMDSClient {
    client: reqwest::blocking::Client,
    token: String,
    token_expiry: SystemTime,
}

#[derive(Debug)]
pub enum MMDSClientError {
    RequestError,
    ResponseSchemaParseError,
}

impl MMDSClient {
    pub fn connect() -> Result<Self, MMDSClientError> {
        let client = reqwest::blocking::Client::new();

        let token_expiry = SystemTime::now() - std::time::Duration::from_secs(60 * 60);

        let mut mmds = MMDSClient {
            client: client.clone(),
            token: String::new(),
            token_expiry,
        };
        mmds.rotate_token()?;
        Ok(mmds)
    }

    fn rotate_token(&mut self) -> Result<(), MMDSClientError> {
        if SystemTime::now() >= self.token_expiry {
            let expiry_seconds = 14400; // 4 hours
            let token_expiry = SystemTime::now() + std::time::Duration::from_secs(expiry_seconds);
            let resp = self
                .client
                .put(format!("http://{}/latest/api/token", MMDS_IP_ADDR))
                .header("X-metadata-token-ttl-seconds", expiry_seconds)
                .send()
                .map_err(|_| MMDSClientError::RequestError)?;
            if resp.status() != reqwest::StatusCode::OK {
                log::debug!(
                    "MMDS token acquisition failed: {} {}",
                    resp.status(),
                    resp.text().unwrap_or_default()
                );
                return Err(MMDSClientError::RequestError);
            }
            let token = resp.text().map_err(|_| MMDSClientError::RequestError)?;
            if token.is_empty() {
                log::debug!("MMDS token is empty");
                return Err(MMDSClientError::ResponseSchemaParseError);
            }
            self.token = token.trim().to_string();
            log::debug!("Rotated MMDS token: \"{}\"", self.token);
            self.token_expiry = token_expiry;
        }
        Ok(())
    }

    pub fn get<T: DeserializeOwned>(&self, resource_path: &str) -> Result<T, MMDSClientError> {
        let resp = self
            .client
            .get(format!("http://{}{}", MMDS_IP_ADDR, resource_path))
            .header("X-metadata-token", &self.token)
            .header("Accept", "application/json")
            .send()
            .map_err(|_| MMDSClientError::RequestError)?;
        if resp.status() != reqwest::StatusCode::OK {
            log::debug!(
                "MMDS GET request failed: {} {}",
                resp.status(),
                resp.text().unwrap_or_default()
            );
            return Err(MMDSClientError::RequestError);
        }
        let out: T = resp
            .json()
            .map_err(|_| MMDSClientError::ResponseSchemaParseError)?;
        Ok(out)
    }
}
