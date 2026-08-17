use clap::Parser;
use std::path::PathBuf;
use std::sync::OnceLock;

static PROFILE: OnceLock<Profile> = OnceLock::new();

#[derive(Parser, Debug)]
#[command(name = "walz", about = "WhatsApp desktop client for Linux")]
pub struct Args {
    /// Profile name to use (creates separate session)
    #[arg(short, long)]
    pub profile: Option<String>,

    /// List available profiles
    #[arg(long)]
    pub list_profiles: bool,

    /// Start minimized to system tray
    #[arg(short, long)]
    pub minimized: bool,
}

#[derive(Debug, Clone)]
pub struct Profile {
    pub name: String,
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub start_minimized: bool,
}

impl Profile {
    pub fn new(name: Option<String>, start_minimized: bool) -> Self {
        let base_data = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("walz");
        let base_config = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("walz");

        match name {
            Some(n) if !n.is_empty() && n != "default" => Profile {
                name: n.clone(),
                data_dir: base_data.join("profiles").join(&n),
                config_dir: base_config.join("profiles").join(&n),
                start_minimized,
            },
            _ => Profile {
                name: "default".to_string(),
                data_dir: base_data,
                config_dir: base_config,
                start_minimized,
            },
        }
    }

    pub fn window_title(&self) -> String {
        if self.name == "default" {
            "Walz".to_string()
        } else {
            format!("Walz ({})", self.name)
        }
    }

    pub fn tray_tooltip(&self) -> String {
        if self.name == "default" {
            "Walz".to_string()
        } else {
            format!("Walz - {}", self.name)
        }
    }
}

pub fn init() -> &'static Profile {
    PROFILE.get_or_init(|| {
        let args = Args::parse();

        if args.list_profiles {
            list_profiles();
            std::process::exit(0);
        }

        Profile::new(args.profile, args.minimized)
    })
}

pub fn get() -> &'static Profile {
    PROFILE.get().expect("Profile not initialized")
}

fn list_profiles() {
    let base_data = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("walz");

    println!("Available profiles:");
    println!("  default");

    let profiles_dir = base_data.join("profiles");
    if profiles_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&profiles_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    if let Some(name) = entry.file_name().to_str() {
                        println!("  {}", name);
                    }
                }
            }
        }
    }
}

pub fn get_all_profiles() -> Vec<String> {
    let mut profiles = vec!["default".to_string()];

    let base_data = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("walz")
        .join("profiles");

    if base_data.exists() {
        if let Ok(entries) = std::fs::read_dir(&base_data) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    if let Some(name) = entry.file_name().to_str() {
                        profiles.push(name.to_string());
                    }
                }
            }
        }
    }

    profiles
}
