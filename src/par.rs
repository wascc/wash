extern crate provider_archive;
use crate::keys::extract_keypair;
use nkeys::KeyPairType;
use provider_archive::*;
use std::fs::File;
use std::io::prelude::*;
use std::path::PathBuf;
use structopt::clap::AppSettings;
use structopt::StructOpt;

#[derive(Debug, StructOpt, Clone)]
#[structopt(
    global_settings(&[AppSettings::ColoredHelp, AppSettings::VersionlessSubcommands]),
    name = "par")]
pub struct ParCli {
    #[structopt(flatten)]
    command: ParCliCommand,
}

#[derive(Debug, Clone, StructOpt)]
enum ParCliCommand {
    /// Build a provider archive file
    #[structopt(name = "create")]
    Create(CreateCommand),
    /// Inspect a provider archive file
    #[structopt(name = "inspect")]
    Inspect(InspectCommand),
    /// Insert a provider into a provider archive file
    #[structopt(name = "insert")]
    Insert(InsertCommand),
}

#[derive(StructOpt, Debug, Clone)]
struct CreateCommand {
    /// Capability contract ID (e.g. wascc:messaging or wascc:keyvalue).
    #[structopt(short = "c", long = "capid")]
    capid: String,

    /// Vendor string to help identify the publisher of the provider (e.g. Redis, Cassandra, waSCC, etc). Not unique.
    #[structopt(short = "v", long = "vendor")]
    vendor: String,

    /// Monotonically increasing revision number
    #[structopt(short = "r", long = "revision")]
    revision: Option<i32>,

    /// Human friendly version string
    #[structopt(name = "version")]
    version: Option<String>,

    /// Location of key files for signing. Defaults to $WASH_KEYS ($HOME/.wash/keys)
    #[structopt(
        short = "d",
        long = "directory",
        env = "WASH_KEYS",
        hide_env_values = true
    )]
    directory: Option<String>,

    /// Path to issuer seed key (account) If this flag is not provided, the will be sourced from $WASH_KEYS ($HOME/.wash/keys) or generated for you if it cannot be found.
    #[structopt(short = "i", long = "issuer")]
    issuer: Option<String>,

    /// Path to subject seed key (service) If this flag is not provided, the will be sourced from $WASH_KEYS ($HOME/.wash/keys) or generated for you if it cannot be found.
    #[structopt(short = "s", long = "subject")]
    subject: Option<String>,

    /// Name of the capability provider
    #[structopt(short = "n", long = "name")]
    name: String,

    /// Architecture of provider binary in format ARCH-OS (e.g. x86_64-linux)
    #[structopt(short = "a", long = "arch")]
    arch: String,

    /// Path to provider binary for populating the archive
    #[structopt(short = "b", long = "binary")]
    binary: String,

    /// Output file path
    #[structopt(short = "o", long = "output")]
    output: Option<String>,
}

#[derive(StructOpt, Debug, Clone)]
struct InspectCommand {
    /// Path to provider archive
    #[structopt(name = "archive")]
    archive: String,
}

#[derive(StructOpt, Debug, Clone)]
struct InsertCommand {
    /// Path to provider archive
    #[structopt(name = "archive")]
    archive: String,

    /// Architecture of binary in format ARCH-OS (e.g. x86_64-linux)
    #[structopt(short = "a", long = "arch")]
    arch: String,

    /// Path to provider binary to insert into archive
    #[structopt(short = "b", long = "binary")]
    binary: String,

    /// Location of key files for signing. Defaults to $WASH_KEYS ($HOME/.wash/keys)
    #[structopt(
        short = "d",
        long = "directory",
        env = "WASH_KEYS",
        hide_env_values = true
    )]
    directory: Option<String>,

    /// Path to issuer seed key (account.) If this flag is not provided, the will be sourced from $WASH_KEYS ($HOME/.wash/keys) or generated for you if it cannot be found.
    #[structopt(short = "i", long = "issuer")]
    issuer: Option<String>,

    /// Path to subject seed key (service). If this flag is not provided, the will be sourced from $WASH_KEYS ($HOME/.wash/keys) or generated for you if it cannot be found.
    #[structopt(short = "s", long = "subject")]
    subject: Option<String>,
}

pub fn handle_command(cli: ParCli) -> Result<()> {
    match cli.command {
        ParCliCommand::Create(cmd) => handle_create(cmd),
        ParCliCommand::Inspect(cmd) => handle_inspect(cmd),
        ParCliCommand::Insert(cmd) => handle_insert(cmd),
    }
}

/// Creates a provider archive using an initial architecture target, provider, and signing keys
fn handle_create(cmd: CreateCommand) -> Result<()> {
    let mut par = ProviderArchive::new(
        &cmd.capid,
        &cmd.name,
        &cmd.vendor,
        cmd.revision,
        cmd.version,
    );

    let mut f = File::open(cmd.binary.clone())?;
    let mut lib = Vec::new();
    f.read_to_end(&mut lib)?;

    let issuer = extract_keypair(
        cmd.issuer,
        cmd.binary.clone(),
        cmd.directory.clone(),
        KeyPairType::Account,
    )?;
    let subject = extract_keypair(
        cmd.subject,
        cmd.binary.clone(),
        cmd.directory,
        KeyPairType::Service,
    )?;

    par.add_library(&cmd.arch, &lib)?;

    let output = match cmd.output {
        Some(path) => path,
        None => format!(
            "{}.par",
            PathBuf::from(cmd.binary.clone())
                .file_stem()
                .unwrap()
                .to_str()
                .unwrap()
                .to_string()
        ),
    };

    match File::create(output.clone()) {
        Ok(mut out) => par.write(&mut out, &issuer, &subject)?,
        Err(e) => println!(
            "Error: {}, please ensure directory {:?} exists",
            e,
            PathBuf::from(output).parent().unwrap()
        ),
    }

    Ok(())
}

/// Loads a provider archive and prints the contents of the claims
fn handle_inspect(cmd: InspectCommand) -> Result<()> {
    let mut buf = Vec::new();
    let mut f = File::open(&cmd.archive)?;
    f.read_to_end(&mut buf)?;

    let archive = ProviderArchive::try_load(&buf)?;
    let claims = archive.claims().unwrap().metadata.unwrap();
    // println!("Name: {}", claims.name.unwrap());
    // println!("Capability contract ID: {}", claims.capid);
    // println!("Vendor: {}", claims.vendor);
    // println!("Supported targets: {:?}", archive.targets());

    use term_table::row::Row;
    use term_table::table_cell::*;
    use term_table::{Table, TableStyle};

    let mut table = Table::new();
    table.max_column_width = 68;
    table.style = TableStyle::extended();

    table.add_row(Row::new(vec![TableCell::new_with_alignment(
        format!("{} - Provider Archive", claims.name.unwrap()),
        2,
        Alignment::Center,
    )]));

    table.add_row(Row::new(vec![
        TableCell::new("Capability Contract ID"),
        TableCell::new_with_alignment(claims.capid, 1, Alignment::Right),
    ]));
    table.add_row(Row::new(vec![
        TableCell::new("Vendor"),
        TableCell::new_with_alignment(claims.vendor, 1, Alignment::Right),
    ]));

    table.add_row(Row::new(vec![TableCell::new_with_alignment(
        "Supported Architecture Targets",
        2,
        Alignment::Center,
    )]));

    table.add_row(Row::new(vec![TableCell::new_with_alignment(
        archive.targets().join("\n"),
        2,
        Alignment::Left,
    )]));

    println!("{}", table.render());

    Ok(())
}

/// Loads a provider archive and attempts to insert an additional provider into it
fn handle_insert(cmd: InsertCommand) -> Result<()> {
    let mut buf = Vec::new();
    let mut f = File::open(cmd.archive.clone())?;
    f.read_to_end(&mut buf)?;

    let mut par = ProviderArchive::try_load(&buf)?;

    let issuer = extract_keypair(
        cmd.issuer,
        cmd.binary.clone(),
        cmd.directory.clone(),
        KeyPairType::Account,
    )?;
    let subject = extract_keypair(
        cmd.subject,
        cmd.binary.clone(),
        cmd.directory,
        KeyPairType::Service,
    )?;

    let mut f = File::open(cmd.binary.clone())?;
    let mut lib = Vec::new();
    f.read_to_end(&mut lib)?;

    par.add_library(&cmd.arch, &lib)?;

    let mut out = File::create(cmd.archive)?;
    par.write(&mut out, &issuer, &subject)?;

    Ok(())
}
