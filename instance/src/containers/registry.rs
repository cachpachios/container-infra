use std::path::Path;

use backoff::ExponentialBackoff;
use oci_spec::image::{Arch, Descriptor, ImageConfiguration, ImageManifest, MediaType, Os};
use oci_spec::{distribution::Reference, image::ImageIndex};
use reqwest::blocking::Client;
use serde::Deserialize;

#[derive(Debug)]
pub enum RegistryErrors {
    NetworkError,
    RegistryResponseError,
    NoCompatibleImageAvailable,
    UnsupportedRegistryImageFormat,
    UnableToParseImageIndex,
    UnableToParseImageManifest,
    UnableToParseImageConfiguration,
    UnableToConstructRuntimeConfig,
    AuthenticationError,
    IOErr,
    ExtractIOError,
}

const SUPPORTED_ARCH: Arch = Arch::Amd64; //TODO: Arm64?

pub fn get_manifest_and_config(
    reference: &Reference,
    auth_token: Option<&str>,
) -> Result<(ImageManifest, ImageConfiguration), RegistryErrors> {
    let index_url = format!(
        "https://{}/v2/{}/manifests/{}",
        reference.resolve_registry(),
        reference.repository(),
        reference.tag().unwrap_or("latest"),
    );
    log::debug!("Pulling index from {}", index_url);
    let index_data: serde_json::Value =
        get_with_backoff(&index_url, auth_token)?
            .json()
            .map_err(|e| {
                log::debug!("Image index response is not JSON: {:?}", e);
                RegistryErrors::UnableToParseImageIndex
            })?;

    let schema_version = index_data
        .get("schemaVersion")
        .and_then(|v| v.as_u64().map(|v| v as u32));

    let manifest = match schema_version {
        Some(oci_spec::image::SCHEMA_VERSION) => {
            let index: ImageIndex = serde_json::from_value(index_data).map_err(|e| {
                log::debug!("UnableToParseImageIndex: {:?}", e);
                RegistryErrors::UnableToParseImageIndex
            })?;

            let compatible_manifest = index
                .manifests()
                .iter()
                .find(|d| {
                    d.platform().as_ref().map_or(false, |p| {
                        *p.architecture() == SUPPORTED_ARCH && *p.os() == Os::Linux
                    })
                })
                .ok_or(RegistryErrors::NoCompatibleImageAvailable)?;

            let manifest_url = format!(
                "https://{}/v2/{}/manifests/{}",
                reference.resolve_registry(),
                reference.repository(),
                compatible_manifest.digest()
            );
            log::debug!("Pulling manifest from {}", manifest_url);
            ImageManifest::from_reader(get(&manifest_url, auth_token)?).map_err(|e| {
                log::debug!("UnableToParseImageManifest: {:?}", e);
                RegistryErrors::UnableToParseImageManifest
            })
        }
        Some(v) => {
            log::error!("Unsupported image format, version {}.", v);
            Err(RegistryErrors::UnsupportedRegistryImageFormat)
        }
        None => {
            log::debug!("No schema version found");
            Err(RegistryErrors::UnableToParseImageIndex)
        }
    }?;

    let config_url = format!(
        "https://{}/v2/{}/blobs/{}",
        reference.resolve_registry(),
        reference.repository(),
        manifest.config().digest()
    );
    log::debug!("Pulling config from {}", config_url);
    let config: ImageConfiguration = ImageConfiguration::from_reader(get(&config_url, auth_token)?)
        .map_err(|e| {
            log::debug!("UnableToParseImageConfiguration: {:?}", e);
            RegistryErrors::UnableToParseImageConfiguration
        })?;
    Ok((manifest, config))
}

pub fn pull_and_extract_layer(
    reference: &Reference,
    layer: &Descriptor,
    output_folder: &Path,
    auth_token: Option<&str>,
) -> Result<usize, RegistryErrors> {
    let blob_url = format!(
        "https://{}/v2/{}/blobs/{}",
        reference.resolve_registry(),
        reference.repository(),
        layer.digest()
    );

    let mut blob_resp = get_with_backoff(&blob_url, auth_token)?;
    let blob_resp_size = blob_resp.content_length().unwrap_or(0);
    extract_layer(&mut blob_resp, &output_folder, layer.media_type())?;
    Ok(blob_resp_size as usize)
}

fn extract_layer(
    blob: &mut impl std::io::Read,
    output_folder: &Path,
    media_type: &MediaType,
) -> Result<(), RegistryErrors> {
    let reader = match media_type {
        MediaType::ImageLayerGzip => {
            let reader = flate2::read::GzDecoder::new(blob);
            Box::new(reader) as Box<dyn std::io::Read>
        }
        MediaType::ImageLayerZstd => {
            let reader = flate2::read::ZlibDecoder::new(blob);
            Box::new(reader) as Box<dyn std::io::Read>
        }
        MediaType::ImageLayer => Box::new(blob) as Box<dyn std::io::Read>,
        _ => return Err(RegistryErrors::IOErr),
    };

    let mut tar = tar::Archive::new(reader);
    tar.set_overwrite(true);
    tar.unpack(output_folder).map_err(|e| {
        log::error!("Unable to extract layer: {}", e);
        RegistryErrors::ExtractIOError
    })
}

pub fn docker_io_oauth(
    scope_type: &str,
    resource_name: &str,
    actions: &[&str],
) -> Result<String, String> {
    let url = format!(
        "https://auth.docker.io/token?service=registry.docker.io&scope={}:{}:{}",
        scope_type,
        resource_name,
        actions.join(",")
    );
    let resp = reqwest::blocking::get(url).map_err(|e| {
        log::debug!("Error while obtaining docker.io token: {:?}", e);
        e.to_string()
    })?;

    #[derive(Deserialize)]
    struct TokenResponse {
        token: String,
    }
    let resp: TokenResponse = resp.json().map_err(|e| e.to_string())?;

    log::debug!("Obtained docker.io token: {}", resp.token);

    Ok(resp.token)
}

fn get_with_backoff(
    url: &str,
    auth_token: Option<&str>,
) -> Result<reqwest::blocking::Response, RegistryErrors> {
    let backoff = ExponentialBackoff {
        initial_interval: std::time::Duration::from_secs(3),
        max_interval: std::time::Duration::from_secs(9),
        max_elapsed_time: Some(std::time::Duration::from_secs(20)),
        multiplier: 1.5,
        ..Default::default()
    };
    let op = || {
        get(url, auth_token).map_err(|e| match e {
            RegistryErrors::NetworkError => backoff::Error::transient(e),
            _ => backoff::Error::permanent(e),
        })
    };
    backoff::retry(backoff, op).map_err(|e| match e {
        backoff::Error::Permanent(inner) | backoff::Error::Transient { err: inner, .. } => inner,
    })
}

fn get(url: &str, auth_token: Option<&str>) -> Result<reqwest::blocking::Response, RegistryErrors> {
    let client = Client::new();
    let mut request = client.get(url);
    if let Some(token) = auth_token {
        request = request.bearer_auth(token);
    }
    let resp = request.send().map_err(|e| {
        log::error!("Network error while accessing {}: {}", url, e);
        RegistryErrors::NetworkError
    })?;
    if resp.status().is_success() {
        Ok(resp)
    } else {
        log::error!(
            "Container repository GET failed: {} - {}",
            resp.status(),
            resp.text().unwrap_or_default()
        );
        Err(RegistryErrors::RegistryResponseError)
    }
}
