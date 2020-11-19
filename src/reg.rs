extern crate oci_distribution;
use oci_distribution::client::*;
use oci_distribution::secrets::RegistryAuth;
use oci_distribution::Reference;
use provider_archive::ProviderArchive;
use spinners::{Spinner, Spinners};
use std::fs::File;
use std::io::prelude::*;
use structopt::clap::AppSettings;
use structopt::StructOpt;
use tokio::runtime::*;

const PROVIDER_ARCHIVE_MEDIA_TYPE: &str = "application/vnd.wascc.provider.archive.layer.v1+par";
const PROVIDER_ARCHIVE_CONFIG_MEDIA_TYPE: &str = "application/vnd.wascc.provider.archive.config";
const PROVIDER_ARCHIVE_FILE_EXTENSION: &str = ".par.gz";
const WASM_MEDIA_TYPE: &str = "application/vnd.module.wasm.content.layer.v1+wasm";
const WASM_CONFIG_MEDIA_TYPE: &str = "application/vnd.wascc.actor.archive.config";
const WASM_FILE_EXTENSION: &str = ".wasm";

const SHOWER_EMOJI: &str = "\u{1F6BF}";

enum SupportedArtifacts {
    Par,
    Wasm,
}

#[derive(Debug, StructOpt, Clone)]
#[structopt(
    global_settings(&[AppSettings::ColoredHelp, AppSettings::VersionlessSubcommands]),
    name = "reg")]
pub struct RegCli {
    #[structopt(flatten)]
    command: RegCliCommand,
}

#[derive(Debug, Clone, StructOpt)]
enum RegCliCommand {
    /// Pull an artifact from an OCI compliant registry
    #[structopt(name = "pull")]
    Pull(PullCommand),
    /// Push an artifact to an OCI compliant registry
    #[structopt(name = "push")]
    Push(PushCommand),
}

#[derive(StructOpt, Debug, Clone)]
struct PullCommand {
    /// URL of artifact
    #[structopt(name = "url")]
    url: String,

    /// OCI username, if omitted anonymous authentication will be used
    #[structopt(
        short = "u",
        long = "user",
        env = "WASH_REG_USER",
        hide_env_values = true
    )]
    user: Option<String>,

    /// OCI password, if omitted anonymous authentication will be used
    #[structopt(
        short = "p",
        long = "password",
        env = "WASH_REG_PASSWORD",
        hide_env_values = true
    )]
    password: Option<String>,

    /// Path to output
    #[structopt(short = "o", long = "output")]
    output: Option<String>,

    /// Digest to verify artifact against
    #[structopt(short = "d", long = "digest")]
    digest: Option<String>,

    /// Allow insecure (HTTP) registry connections
    #[structopt(long = "insecure")]
    insecure: bool,

    /// Allow latest artifact tags
    #[structopt(long = "allow-latest")]
    allow_latest: bool,
}

#[derive(StructOpt, Debug, Clone)]
struct PushCommand {
    /// URL to push artifact to
    #[structopt(name = "url")]
    url: String,

    /// Path to artifact to push
    #[structopt(name = "artifact")]
    artifact: String,

    /// OCI username, if omitted anonymous authentication will be used
    #[structopt(
        short = "u",
        long = "user",
        env = "WASH_REG_USER",
        hide_env_values = true
    )]
    user: Option<String>,

    /// OCI password, if omitted anonymous authentication will be used
    #[structopt(
        short = "p",
        long = "password",
        env = "WASH_REG_PASSWORD",
        hide_env_values = true
    )]
    password: Option<String>,

    /// Path to config file, if omitted will default to a blank configuration
    #[structopt(short = "c", long = "config")]
    config: Option<String>,

    /// Allow insecure (HTTP) registry connections
    #[structopt(long = "insecure")]
    insecure: bool,

    /// Allow latest artifact tags
    #[structopt(long = "allow-latest")]
    allow_latest: bool,
}

pub fn handle_command(cli: RegCli) -> Result<(), Box<dyn ::std::error::Error>> {
    match cli.command {
        RegCliCommand::Pull(cmd) => handle_pull(cmd),
        RegCliCommand::Push(cmd) => handle_push(cmd),
    }
}

fn handle_pull(cmd: PullCommand) -> Result<(), Box<dyn ::std::error::Error>> {
    let image: Reference = cmd.url.parse().unwrap();

    if image.tag().unwrap_or("latest") == "latest" && !cmd.allow_latest {
        return Err(
            "Pulling artifacts with tag 'latest' is prohibited. This can be overriden with a flag"
                .into(),
        );
    };

    let mut client = Client::new(ClientConfig {
        protocol: if cmd.insecure {
            ClientProtocol::Http
        } else {
            ClientProtocol::Https
        },
    });

    let auth = match (cmd.user, cmd.password) {
        (Some(user), Some(password)) => RegistryAuth::Basic(user, password),
        _ => RegistryAuth::Anonymous,
    };

    let sp = Spinner::new(
        Spinners::Dots12,
        format!(" Downloading {} ...", image.whole()),
    );

    // Asynchronous code from the oci-distribution crate must run on the tokio runtime
    let mut rt = Runtime::new()?;
    let image_data = rt.block_on(client.pull(
        &image,
        &auth,
        vec![PROVIDER_ARCHIVE_MEDIA_TYPE, WASM_MEDIA_TYPE],
    ))?;

    sp.message(format!(" Validating {} ...", image.whole()));

    // Reformatting digest in case the sha256: prefix is left off
    let digest = match cmd.digest {
        Some(d) if d.starts_with("sha256:") => Some(d),
        Some(d) => Some(format!("sha256:{}", d)),
        None => None,
    };

    match (digest, image_data.digest) {
        (Some(digest), Some(image_digest)) if digest != image_digest => {
            Err("Image digest did not match provided digest, aborting")
        }
        _ => Ok(()),
    }?;

    let artifact = image_data
        .layers
        .iter()
        .map(|l| l.data.clone())
        .flatten()
        .collect::<Vec<_>>();

    let file_extension = match validate_artifact(&artifact, image.repository())? {
        SupportedArtifacts::Par => PROVIDER_ARCHIVE_FILE_EXTENSION,
        SupportedArtifacts::Wasm => WASM_FILE_EXTENSION,
    };

    // Output to provided file, or use artifact_name.file_extension
    let outfile = cmd.output.unwrap_or(format!(
        "{}{}",
        image
            .repository()
            .to_string()
            .split('/')
            .collect::<Vec<_>>()
            .pop()
            .unwrap()
            .to_string(),
        file_extension
    ));
    let mut f = File::create(outfile.clone())?;
    f.write_all(&artifact)?;

    sp.stop();
    println!(
        "\n{} Successfully pulled and validated {}",
        SHOWER_EMOJI, outfile
    );

    Ok(())
}

/// Helper function to determine artifact type and validate that it is
/// a valid artifact of that type
fn validate_artifact(
    artifact: &[u8],
    name: &str,
) -> Result<SupportedArtifacts, Box<dyn ::std::error::Error>> {
    match validate_actor_module(artifact, name) {
        Ok(_) => Ok(SupportedArtifacts::Wasm),
        Err(_) => match validate_provider_archive(artifact, name) {
            Ok(_) => Ok(SupportedArtifacts::Par),
            Err(_) => Err("Unsupported artifact type".into()),
        },
    }
}

/// Attempts to inspect the claims of an actor module
/// Will fail without actor claims, or if the artifact is invalid
fn validate_actor_module(
    artifact: &[u8],
    module: &str,
) -> Result<(), Box<dyn ::std::error::Error>> {
    match wascap::wasm::extract_claims(&artifact) {
        Ok(Some(_token)) => Ok(()),
        Ok(None) => Err(format!("No capabilities discovered in actor module : {}", &module).into()),
        Err(e) => Err(Box::new(e)),
    }
}

/// Attempts to unpack a provider archive
/// Will fail without claims or if the archive is invalid
fn validate_provider_archive(
    artifact: &[u8],
    archive: &str,
) -> Result<(), Box<dyn ::std::error::Error>> {
    match ProviderArchive::try_load(artifact) {
        Ok(_par) => Ok(()),
        Err(_e) => Err(format!("Invalid provider archive : {}", archive).into()),
    }
}

fn handle_push(cmd: PushCommand) -> Result<(), Box<dyn ::std::error::Error>> {
    let image: Reference = cmd.url.parse().unwrap();

    if image.tag().unwrap() == "latest" && !cmd.allow_latest {
        return Err(
            "Pushing artifacts with tag 'latest' is prohibited. This can be overriden with a flag"
                .into(),
        );
    };

    let sp = Spinner::new(Spinners::Dots12, format!(" Loading {} ...", cmd.artifact));
    let mut config_buf = vec![];
    match cmd.config {
        Some(config_file) => {
            let mut f = File::open(config_file)?;
            f.read_to_end(&mut config_buf)?;
        }
        None => {
            // If no config provided, send blank config
            config_buf = b"{}".to_vec();
        }
    };

    let mut artifact_buf = vec![];
    let mut f = File::open(cmd.artifact.clone())?;
    f.read_to_end(&mut artifact_buf)?;

    sp.message(format!(" Verifying {} ...", cmd.artifact));

    let (artifact_media_type, config_media_type) =
        match validate_artifact(&artifact_buf, &cmd.artifact)? {
            SupportedArtifacts::Wasm => (WASM_MEDIA_TYPE, WASM_CONFIG_MEDIA_TYPE),
            SupportedArtifacts::Par => (
                PROVIDER_ARCHIVE_MEDIA_TYPE,
                PROVIDER_ARCHIVE_CONFIG_MEDIA_TYPE,
            ),
        };

    let image_data = ImageData {
        layers: vec![ImageLayer {
            data: artifact_buf,
            media_type: artifact_media_type.to_string(),
        }],
        digest: None,
    };

    let mut client = Client::new(ClientConfig {
        protocol: if cmd.insecure {
            ClientProtocol::Http
        } else {
            ClientProtocol::Https
        },
    });

    let auth = match (cmd.user, cmd.password) {
        (Some(user), Some(password)) => RegistryAuth::Basic(user, password),
        _ => RegistryAuth::Anonymous,
    };

    sp.message(format!(
        " Pushing {} to {} ...",
        cmd.artifact,
        image.whole()
    ));

    // Asynchronous code from the oci-distribution crate must run on the tokio runtime
    let mut rt = Runtime::new()?;
    rt.block_on(client.push(
        &image,
        &image_data,
        &config_buf,
        config_media_type,
        &auth,
        None,
    ))?;

    sp.stop();
    println!(
        "\n{} Successfully validated and pushed to {}",
        SHOWER_EMOJI,
        image.whole()
    );

    Ok(())
}
