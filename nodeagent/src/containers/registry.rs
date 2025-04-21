use std::path::Path;

use oci_spec::image::{Arch, Descriptor, ImageConfiguration, ImageManifest, MediaType, Os};
use oci_spec::{distribution::Reference, image::ImageIndex};
use reqwest::blocking::Client;
use serde::Deserialize;

#[derive(Debug)]
pub enum RegistryErrors {
    NetworkError,
    NoCompatibleImageAvailable,
    UnableToParse,
    AuthenticationError,
    IOErr,
}

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
    let req = get(&index_url, auth_token)?;
    let index: ImageIndex = req.json().map_err(|_| RegistryErrors::UnableToParse)?;

    let compatible_manifest = index
        .manifests()
        .iter()
        .find(|d| {
            d.platform().as_ref().map_or(false, |p| {
                *p.architecture() == Arch::Amd64 && *p.os() == Os::Linux
            })
        })
        .ok_or(RegistryErrors::NoCompatibleImageAvailable)?;

    let manifest_url = format!(
        "https://{}/v2/{}/manifests/{}",
        reference.resolve_registry(),
        reference.repository(),
        compatible_manifest.digest()
    );

    let manifest: ImageManifest = ImageManifest::from_reader(get(&manifest_url, auth_token)?)
        .map_err(|_| RegistryErrors::UnableToParse)?;

    let config_url = format!(
        "https://{}/v2/{}/blobs/{}",
        reference.resolve_registry(),
        reference.repository(),
        manifest.config().digest()
    );
    let config_resp = get(&config_url, auth_token)?;
    let config: ImageConfiguration =
        ImageConfiguration::from_reader(config_resp).map_err(|_| RegistryErrors::UnableToParse)?;

    Ok((manifest, config))
}

pub fn pull_and_extract_layer(
    reference: &Reference,
    layer: &Descriptor,
    output_folder: &Path,
    auth_token: Option<&str>,
) -> Result<(), RegistryErrors> {
    let blob_url = format!(
        "https://{}/v2/{}/blobs/{}",
        reference.resolve_registry(),
        reference.repository(),
        layer.digest()
    );

    let mut blob_resp = get(&blob_url, auth_token).map_err(|_| RegistryErrors::NetworkError)?;
    extract_layer(&mut blob_resp, &output_folder, layer.media_type())
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
    tar.unpack(output_folder)
        .map_err(|_| RegistryErrors::IOErr)?;
    Ok(())
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
    let resp = reqwest::blocking::get(url).map_err(|e| e.to_string())?;

    #[derive(Deserialize)]
    struct TokenResponse {
        token: String,
    }
    let resp: TokenResponse = resp.json().map_err(|e| e.to_string())?;

    Ok(resp.token)
}

fn get(url: &str, auth_token: Option<&str>) -> Result<reqwest::blocking::Response, RegistryErrors> {
    let client = Client::new();
    let mut request = client.get(url);
    if let Some(token) = auth_token {
        request = request.bearer_auth(token);
    }
    let resp = request.send().map_err(|_| RegistryErrors::NetworkError)?;
    if resp.status().is_success() {
        Ok(resp)
    } else {
        log::error!(
            "Repository GET failed: {} - {}",
            resp.status(),
            resp.text().unwrap_or_default()
        );
        Err(RegistryErrors::NetworkError)
    }
}
