use clap::Parser;

#[derive(Parser, Debug, clap::ValueEnum, Clone)]
pub enum EntryOutput {
    #[clap(name = "id")]
    Id,
    #[clap(name = "path")]
    Path,
    #[clap(name = "both")]
    Both,
}
