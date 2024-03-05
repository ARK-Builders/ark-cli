use clap::Parser;

#[derive(Parser, Debug, Clone)]
pub enum EntryOutput {
    Id,
    Path,
    Both,
}
