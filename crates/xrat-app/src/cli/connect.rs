use clap::Args;

#[derive(Debug, Args)]
#[command(about = "Start a managed proxy runtime for a stored config.")]
pub struct ConnectArgs {
    #[arg(
        help = "Config ID, ref prefix, or 'latest' to start as the active local proxy session. Defaults to the last connected config if omitted."
    )]
    pub id: Option<String>,

    #[arg(long = "json", help = "Print the result as JSON.")]
    pub json: bool,
}
