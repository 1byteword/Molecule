mod handlers;
mod models;
mod storage;
mod encryption;
mod access_control;

use actix_web::{App, HttpServer, HttpResponse, Responder, web};
use std::sync::Mutex;
use std::collections::HashMap;
use handlers::{store, load, AppState};
use log::info;
use clap::{Parser, Subcommand};
use storage::{ensure_dir_exists, save_to_file, load_from_file};
use encryption::{generate_key, encrypt, decrypt};
use access_control::AccessControl;
use uuid::Uuid;
use std::fs;
use std::fs::File;
use std::io::{Write, Read};

use bcrypt::{hash, verify, DEFAULT_COST};

const USER_ID_FILE: &str = "user_id.txt";
const KEY_FILE: &str = "encryption_key.bin";

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = "This is a simple API server that securely stores and loads data.")]
struct Args {
    #[clap(subcommand)]
    command: Command,
}

struct User {
    username: String,
    password_hash: String,
}

#[derive(Subcommand, Debug)]
enum Command {
    Serve {
        // bind server
        #[clap(short, long, default_value = "127.0.0.1:8000")]
        address: String,
    },

    // data tokenizer
    Store {
        #[clap(short, long)]
        data: Vec<String>,
    },

    // data detokenizer
    Load {
        #[clap(short, long)]
        data: Vec<String>,
    },
}

fn hash_password(password: &str) -> Result<String, bcrypt::BcryptError> {
    hash(password, DEFAULT_COST)
}

fn register_user(username: String, password: String) -> Result<(), String> {
    println!("Registering user...");
    let password_hash = hash_password(&password).map_err(|e| e.to_string())?;

    println!("User {} registered successfully.", username);

    Ok(())
}

fn authenticate_user(username: &str, password: &str) -> Result<bool, String> {
    let user = User {
        username: username.to_string(),
        password_hash: hash_password(password).unwrap(),
    };

    match verify(password, &user.password_hash) {
        Ok(matching) => Ok(matching),
        Err(e) => Err(e.to_string()),
    }
}

async fn login(info: web::Json<User>) -> impl Responder {
    if authenticate_user(&info.username, &info.password_hash).unwrap_or(false) {
        HttpResponse::Ok().body("Login successful")
    } else {
        HttpResponse::BadRequest().body("Login failed. Invalid username or password.")
    }
}

fn get_or_create_user_id() -> Uuid {
    if let Ok(contents) = fs::read_to_string(USER_ID_FILE) {
        if let Ok(uuid) = Uuid::parse_str(contents.trim()) {
            return uuid;
        }
    }
    let user_id = Uuid::new_v4();
    fs::write(USER_ID_FILE, user_id.to_string()).expect("Unable to write user_id file");
    user_id
}

fn get_or_create_key() -> Vec<u8> {
    if let Ok(mut file) = File::open(KEY_FILE) {
        let mut key = vec![0; 32];
        file.read_exact(&mut key).expect("Unable to read key file");
        return key;
    }
    let key = generate_key();
    let mut file = File::create(KEY_FILE).expect("Unable to create key file");
    file.write_all(&key).expect("Unable to write key file");
    key
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();
    info!("Starting your Barnyard...");

    let args = Args::parse();
    let base_dir = "secure_data";
    ensure_dir_exists(&base_dir).unwrap();

    let user_id = get_or_create_user_id();
    let key = get_or_create_key();

    let mut access_control = AccessControl::new();
    access_control.grant_access(user_id, format!("{}/my_secret_document.txt", base_dir));

    let path = format!("{}/my_secret_document.txt", base_dir);

    match args.command {
        Command::Serve { address } => {
            let app_data = web::Data::new(AppState {
                tokens: Mutex::new(std::collections::HashMap::new()),
            });
        
            HttpServer::new(move || {
                App::new()
                    .app_data(app_data.clone())
                    .service(web::resource("/store").route(web::post().to(store)))
                    .service(web::resource("/load").route(web::post().to(load)))
            })
            .bind("127.0.0.1:8080")?
            .run()
            .await
        }

        Command::Store { data } => {
            let data_str = data.join(" ");
            let encrypted_data = encrypt(data_str.as_bytes(), &key);
            save_to_file(&path, &encrypted_data).unwrap();
            access_control.grant_access(user_id, path.clone());

            info!("Tokenized data and saved to {}", path);
            println!("Your data has been tokenized and saved to {}", path);
            Ok(())
        }

        Command::Load { data: _ } => {
            if access_control.has_access(user_id, path.as_str()) {
                let loaded_data = load_from_file(&path).unwrap();
                match decrypt(&loaded_data, &key) {
                    Ok(decrypted_data) => {
                        let decrypted_str = String::from_utf8(decrypted_data.clone()).unwrap();
                        info!("Retrieved data: {:?}", decrypted_str);
                        println!("Decrypted retrieved data: {:?}", decrypted_str);
                    }
                    Err(e) => {
                        println!("Failed to decrypt data: {}", e);
                    }
                }
            } else {
                println!("Access denied.");
            }
            Ok(())
        }
    }
}