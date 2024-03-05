use clap::Parser;

#[derive(Parser, Debug, clap::ValueEnum, Clone)]
pub enum Sort {
    #[clap(name = "asc")]
    Asc,
    #[clap(name = "desc")]
    Desc,
}
