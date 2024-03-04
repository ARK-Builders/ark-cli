use std::fs::{create_dir_all, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::str::FromStr;

use arklib::id::ResourceId;
use arklib::pdf::PDFQuality;
use arklib::{app_id, provide_index};

use chrono::prelude::DateTime;
use chrono::Utc;

use clap::Parser;

use fs_extra::dir::{self, CopyOptions};

use home::home_dir;

use crate::models::cli::{Command, FileCommand, Link, StorageCommand};
use crate::models::entry::EntryOutput;
use crate::models::format::Format;
use crate::models::sort::Sort;
use crate::models::storage::{Storage, StorageType};

use util::{
    discover_roots, monitor_index, provide_root, read_storage_value,
    storages_exists, timestamp, translate_storage,
};

mod commands;
mod models;
mod util;

const ARK_CONFIG: &str = ".config/ark";
const ARK_BACKUPS_PATH: &str = ".ark-backups";
const ROOTS_CFG_FILENAME: &str = "roots";

struct StorageEntry {
    path: Option<PathBuf>,
    resource: Option<ResourceId>,
    content: Option<String>,
    tags: Option<Vec<String>>,
    scores: Option<u32>,
    datetime: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), String> {
    env_logger::init();

    let args = models::cli::Cli::parse();

    let app_id_dir = home_dir()
        .ok_or_else(|| "Couldn't retrieve home directory!".to_owned())?;
    let ark_dir = app_id_dir.join(".ark");
    if !ark_dir.exists() {
        std::fs::create_dir(&ark_dir)
            .map_err(|e| format!("Couldn't create .ark directory: {}", e))?;
    }

    println!("Loading app id at {}...", ark_dir.display());

    let _ = app_id::load(ark_dir).map_err(|e| {
        eprintln!("Couldn't load app id: {}", e);
        std::process::exit(1);
    });

    match &args.command {
        Command::List {
            entry,
            entry_id,
            entry_path,

            root_dir,
            modified,
            tags,
            scores,
            sort,
            filter,
        } => {
            let root = provide_root(root_dir)?;

            let entry_output =
                match (entry, entry_id, entry_path) {
                    (Some(e), false, false) => *e,
                    (None, true, false) => EntryOutput::Id,
                    (None, false, true) => EntryOutput::Path,
                    (None, true, true) => EntryOutput::Both,
                    (None, false, false) => EntryOutput::Link,
                    _ => return Err(
                        "You can't use both entry and entry_id or entry_path"
                            .to_owned(),
                    ),
                };

            let mut storage_entries: Vec<StorageEntry> = provide_index(&root)
                .map_err(|_| "Could not provide index".to_owned())?
                .read()
                .map_err(|_| "Could not read index".to_owned())?
                .path2id
                .iter()
                .filter_map(|(path, resource)| {
                    let tags = if *tags {
                        Some(
                            read_storage_value(
                                &root,
                                "tags",
                                &resource.id.to_string(),
                                &None,
                            )
                            .map_or(vec![], |s| {
                                s.split(',')
                                    .map(|s| s.trim().to_string())
                                    .collect::<Vec<_>>()
                            }),
                        )
                    } else {
                        None
                    };

                    let scores = if *scores {
                        Some(
                            read_storage_value(
                                &root,
                                "scores",
                                &resource.id.to_string(),
                                &None,
                            )
                            .map_or(0, |s| s.parse::<u32>().unwrap_or(0)),
                        )
                    } else {
                        None
                    };

                    let datetime = if *modified {
                        let format = "%b %e %H:%M %Y";
                        Some(
                            DateTime::<Utc>::from(resource.modified)
                                .format(format)
                                .to_string(),
                        )
                    } else {
                        None
                    };

                    let (path, resource, content) = match entry_output {
                        EntryOutput::Both => (
                            Some(path.to_owned().into_path_buf()),
                            Some(resource.id),
                            None,
                        ),
                        EntryOutput::Path => {
                            (Some(path.to_owned().into_path_buf()), None, None)
                        }
                        EntryOutput::Id => (None, Some(resource.id), None),
                        EntryOutput::Link => match File::open(&path) {
                            Ok(mut file) => {
                                let mut contents = String::new();
                                match file.read_to_string(&mut contents) {
                                    Ok(_) => (None, None, Some(contents)),
                                    Err(_) => return None,
                                }
                            }
                            Err(_) => return None,
                        },
                    };

                    Some(StorageEntry {
                        path,
                        resource,
                        content,
                        tags,
                        scores,
                        datetime,
                    })
                })
                .collect::<Vec<_>>();

            match sort {
                Some(Sort::Asc) => {
                    storage_entries.sort_by(|a, b| a.datetime.cmp(&b.datetime))
                }

                Some(Sort::Desc) => {
                    storage_entries.sort_by(|a, b| b.datetime.cmp(&a.datetime))
                }
                None => (),
            };

            if let Some(filter) = filter {
                storage_entries.retain(|entry| {
                    entry
                        .tags
                        .as_ref()
                        .map(|tags| tags.contains(filter))
                        .unwrap_or(false)
                });
            }

            let no_tags = "NO_TAGS";
            let no_scores = "NO_SCORE";

            let longest_path = storage_entries
                .iter()
                .map(|entry| {
                    if let Some(path) = entry.path.as_ref() {
                        path.display().to_string().len()
                    } else {
                        0
                    }
                })
                .max_by(|a, b| a.cmp(b))
                .unwrap_or(0);

            let longest_id = storage_entries.iter().fold(0, |acc, entry| {
                if let Some(resource) = &entry.resource {
                    let id_len = resource.to_string().len();
                    if id_len > acc {
                        id_len
                    } else {
                        acc
                    }
                } else {
                    acc
                }
            });

            let longest_tags = storage_entries.iter().fold(0, |acc, entry| {
                let tags_len = entry
                    .tags
                    .as_ref()
                    .map(|tags| {
                        if tags.is_empty() {
                            no_tags.len()
                        } else {
                            tags.join(", ").len()
                        }
                    })
                    .unwrap_or(0);
                if tags_len > acc {
                    tags_len
                } else {
                    acc
                }
            });

            let longest_scores =
                storage_entries.iter().fold(0, |acc, entry| {
                    let scores_len = entry
                        .scores
                        .as_ref()
                        .map(|score| {
                            if *score == 0 {
                                no_scores.len()
                            } else {
                                score.to_string().len()
                            }
                        })
                        .unwrap_or(0);
                    if scores_len > acc {
                        scores_len
                    } else {
                        acc
                    }
                });

            let longest_datetime =
                storage_entries.iter().fold(0, |acc, entry| {
                    let datetime_len = entry
                        .datetime
                        .as_ref()
                        .map(|datetime| datetime.len())
                        .unwrap_or(0);
                    if datetime_len > acc {
                        datetime_len
                    } else {
                        acc
                    }
                });

            let longest_content =
                storage_entries.iter().fold(0, |acc, entry| {
                    let content_len = entry
                        .content
                        .as_ref()
                        .map(|content| content.len())
                        .unwrap_or(0);
                    if content_len > acc {
                        content_len
                    } else {
                        acc
                    }
                });

            for entry in &storage_entries {
                let mut output = String::new();

                if let Some(content) = &entry.content {
                    output.push_str(&format!(
                        "{:width$} ",
                        content,
                        width = longest_content
                    ));
                }

                if let Some(path) = &entry.path {
                    output.push_str(&format!(
                        "{:width$} ",
                        path.display(),
                        width = longest_path
                    ));
                }

                if let Some(resource) = &entry.resource {
                    output.push_str(&format!(
                        "{:width$} ",
                        resource.to_string(),
                        width = longest_id
                    ));
                }

                if let Some(tags) = &entry.tags {
                    let tags_out = if tags.is_empty() {
                        no_tags.to_owned()
                    } else {
                        tags.join(", ")
                    };

                    output.push_str(&format!(
                        "{:width$} ",
                        tags_out,
                        width = longest_tags
                    ));
                }

                if let Some(scores) = &entry.scores {
                    let scores_out = if *scores == 0 {
                        no_scores.to_owned()
                    } else {
                        scores.to_string()
                    };

                    output.push_str(&format!(
                        "{:width$} ",
                        scores_out,
                        width = longest_scores
                    ));
                }

                if let Some(datetime) = &entry.datetime {
                    output.push_str(&format!(
                        "{:width$} ",
                        datetime,
                        width = longest_datetime
                    ));
                }

                println!("{}", output);
            }
        }
        Command::Backup { roots_cfg } => {
            let timestamp = timestamp().as_secs();
            let backup_dir = home_dir()
                .ok_or_else(|| "Couldn't retrieve home directory!".to_owned())?
                .join(ARK_BACKUPS_PATH)
                .join(timestamp.to_string());

            if backup_dir.is_dir() {
                println!("Wait at least 1 second, please!");
                std::process::exit(0)
            }

            println!("Preparing backup:");
            let roots = discover_roots(roots_cfg)?;

            let (valid, invalid): (Vec<PathBuf>, Vec<PathBuf>) = roots
                .into_iter()
                .partition(|root| storages_exists(root));

            if !invalid.is_empty() {
                println!("These folders don't contain any storages:");
                invalid
                    .into_iter()
                    .for_each(|root| println!("\t{}", root.display()));
            }

            if valid.is_empty() {
                println!("Nothing to backup. Bye!");
                std::process::exit(0)
            }

            create_dir_all(&backup_dir)
                .map_err(|_| "Couldn't create backup directory!".to_owned())?;

            let mut roots_cfg_backup =
                File::create(backup_dir.join(ROOTS_CFG_FILENAME))
                    .map_err(|_| "Couldn't backup roots config!".to_owned())?;

            valid.iter().for_each(|root| {
                let _ = writeln!(roots_cfg_backup, "{}", root.display())
                    .map_err(|_| {
                        "Couldn't write to roots config backup!".to_owned()
                    });
            });

            println!("Performing backups:");
            valid
                .into_iter()
                .enumerate()
                .for_each(|(i, root)| {
                    println!("\tRoot {}", root.display());
                    let storage_backup = backup_dir.join(i.to_string());

                    let mut options = CopyOptions::new();
                    options.overwrite = true;
                    options.copy_inside = true;

                    let result = dir::copy(
                        root.join(arklib::ARK_FOLDER),
                        storage_backup,
                        &options,
                    );

                    if let Err(e) = result {
                        println!("\t\tFailed to copy storages!\n\t\t{}", e);
                    }
                });

            println!("Backup created:\n\t{}", backup_dir.display());
        }
        Command::Collisions { root_dir } => monitor_index(root_dir, None)?,
        Command::Monitor { root_dir, interval } => {
            let millis = interval.unwrap_or(1000);
            monitor_index(root_dir, Some(millis))?
        }
        Command::Render { path, quality } => {
            let filepath = path.to_owned().unwrap();
            let quality = match quality.to_owned().unwrap().as_str() {
                "high" => PDFQuality::High,
                "medium" => PDFQuality::Medium,
                "low" => PDFQuality::Low,
                _ => return Err("unknown render option".to_owned()),
            };
            let buf = File::open(&filepath).unwrap();
            let dest_path = filepath.with_file_name(
                filepath
                    .file_stem()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned()
                    + ".png",
            );
            let img = arklib::pdf::render_preview_page(buf, quality);
            img.save(dest_path).unwrap();
        }
        Command::Link(link) => match &link {
            Link::Create {
                root_dir,
                url,
                title,
                desc,
            } => {
                let root = provide_root(root_dir)?;
                let url = url
                    .as_ref()
                    .ok_or_else(|| "Url was not provided".to_owned())?;
                let title = title
                    .as_ref()
                    .ok_or_else(|| "Title was not provided".to_owned())?;

                println!("Saving link...");

                match commands::link::create_link(
                    &root,
                    url,
                    title,
                    desc.to_owned(),
                )
                .await
                {
                    Ok(_) => {
                        println!("Link saved successfully!");
                    }
                    Err(e) => println!("{}", e),
                }
            }

            Link::Load {
                root_dir,
                file_path,
                id,
            } => {
                let root = provide_root(root_dir)?;
                let link = commands::link::load_link(&root, file_path, id);

                if let Ok(link) = link {
                    println!("Link data:\n{:?}", link);
                } else {
                    return Err("Could not load link".to_owned());
                }
            }
        },
        Command::File(file) => match &file {
            FileCommand::Append {
                root_dir,
                storage,
                id,
                content,
                format,
                type_,
            } => {
                let (file_path, storage_type) =
                    translate_storage(&Some(root_dir.to_owned()), storage)
                        .ok_or_else(|| {
                            "Could not find storage folder".to_owned()
                        })?;

                let storage_type = storage_type.unwrap_or(match type_ {
                    Some(t) => *t,
                    None => StorageType::File,
                });

                let format = format.unwrap_or(Format::Raw);

                let mut storage = Storage::new(file_path, storage_type)
                    .map_err(|_| "Could not create storage".to_owned())?;

                let resource_id = ResourceId::from_str(id)
                    .map_err(|_| "Could not parse id".to_owned())?;

                storage
                    .append(resource_id, content, format)
                    .map_err(|_| {
                        "Could not append content to storage".to_owned()
                    })?;
            }

            FileCommand::Insert {
                root_dir,
                storage,
                id,
                content,
                format,
                type_,
            } => {
                let (file_path, storage_type) =
                    translate_storage(&Some(root_dir.to_owned()), storage)
                        .ok_or_else(|| {
                            "Could not find storage folder".to_owned()
                        })?;

                let storage_type = storage_type.unwrap_or(match type_ {
                    Some(t) => *t,
                    None => StorageType::File,
                });

                let format = format.unwrap_or(Format::Raw);

                let mut storage = Storage::new(file_path, storage_type)
                    .map_err(|_| "Could not create storage".to_owned())?;

                let resource_id = ResourceId::from_str(id)
                    .map_err(|_| "Could not parse id".to_owned())?;

                storage
                    .insert(resource_id, content, format)
                    .map_err(|_| {
                        "Could not insert content to storage".to_owned()
                    })?;
            }

            FileCommand::Read {
                root_dir,
                storage,
                id,
                type_,
            } => {
                let (file_path, storage_type) =
                    translate_storage(&Some(root_dir.to_owned()), storage)
                        .ok_or_else(|| {
                            "Could not find storage folder".to_owned()
                        })?;

                let storage_type = storage_type.unwrap_or(match type_ {
                    Some(t) => *t,
                    None => StorageType::File,
                });

                let mut storage = Storage::new(file_path, storage_type)
                    .map_err(|_| "Could not create storage".to_owned())?;

                let resource_id = ResourceId::from_str(id)
                    .map_err(|_| "Could not parse id".to_owned())?;

                let output = storage.read(resource_id);

                if let Ok(output) = output {
                    println!("{}", output);
                } else {
                    return Err(
                        "Could not read content from storage".to_owned()
                    );
                }
            }
        },
        Command::Storage(cmd) => match &cmd {
            StorageCommand::List {
                root_dir,
                storage,
                type_,
                versions,
            } => {
                let storage = storage
                    .as_ref()
                    .ok_or_else(|| "Storage was not provided".to_owned())?;

                let versions = versions.unwrap_or(false);

                let (file_path, storage_type) =
                    translate_storage(root_dir, storage).ok_or_else(|| {
                        "Could not find storage folder".to_owned()
                    })?;

                let storage_type = storage_type.unwrap_or(match type_ {
                    Some(t) => *t,
                    None => StorageType::File,
                });

                let mut storage = Storage::new(file_path, storage_type)
                    .map_err(|_| "Could not create storage".to_owned())?;

                storage
                    .load()
                    .map_err(|_| "Could not load storage".to_owned())?;

                let output = storage
                    .list(versions)
                    .map_err(|_| "Could not list storage content".to_owned())?;

                println!("{}", output);
            }
        },
    };

    Ok(())
}
