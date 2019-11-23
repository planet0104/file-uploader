use std::collections::HashMap;

#[macro_use] extern crate tera;
#[macro_use] extern crate lazy_static;
use actix_multipart::Multipart;
use actix_web::{error, middleware, web, App, Error, HttpResponse, HttpServer};
use futures::{future::Either, Future, Stream};

mod form_data;
use form_data::*;
const CONF_FILE_NAME: &str = "conf.ini";
const DEFAULT_MAX_FILE_SIZE:usize = 10 * 1024 * 1024;

use tera::{Tera, Context};

lazy_static! {
    pub static ref TEMPLATES: Tera = {
        compile_templates!("templates/**/*")
    };
}

#[derive(Clone, Debug)]
pub struct Config{
    pub port: String,
    pub path: String,
    pub max_file_size: usize,
    pub pwd: String,
    pub uri: String
}

impl Default for Config {
    fn default() -> Self {
        Config{
            port: String::from("5051"),
            path: String::from("./"),
            uri: String::from("/file_uploader"),
            max_file_size: DEFAULT_MAX_FILE_SIZE,
            pwd: String::from("123456")
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub index: String,
    pub conf: Config,
}

pub fn upload(
    multipart: Multipart,
    state: web::Data<AppState>,
) -> impl Future<Item = HttpResponse, Error = Error> {
    let pwd = state.conf.pwd.clone();
    let mut path = std::path::PathBuf::new();
    path.push(state.conf.path.clone());
    multipart
        .map_err(error::ErrorInternalServerError)
        .map(|field|{
            let key = field.content_disposition().and_then(|d|{
                d.get_name().and_then(|name|{
                    Some(String::from(name))
                })
            }).unwrap_or(String::new());
            //应该在此处判断密码，密码错误不生成临时文件
            if key == "file"{
                Either::B(save_file(field, key).into_stream())
            }else{
                Either::A(get_string(field, key).into_stream())
            }
        })
        .flatten()
        .collect()
        //修改Future的返回类型
        .map(move |fields|{
            let mut fields_map = HashMap::new();
            for f in fields{
                let key = f.key.clone();
                fields_map.insert(key, f.data.clone());
            }

            let mut error_msg = None;
            let mut file_info = None;
            if let Some(FieldData::Paramter(p)) = fields_map.remove("pwd"){
                if p.value != pwd{
                    error_msg = Some("密码错误!");
                }
            }else{
                error_msg = Some("请输入密码!");
            }
            if let Some(FieldData::File(f)) = fields_map.remove("file"){
                file_info = Some(f);
            }else{
                error_msg = Some("请选择文件!");
            }
            (error_msg, file_info)
        })
        .and_then(|(err, file_info)|{
            //线程池中复制文件
            web::block(move || {
                if err.is_some(){
                    return Ok(err.unwrap().to_string());
                }
                match file_info{
                    Some(file_info) => {
                        path.push(file_info.file_name);
                        std::fs::copy(file_info.temp_file, path)
                        .map_err(|err|{
                            format!("文件复制失败 {:?}", err)
                        }).map(|_|{
                            String::from("文件上传成功")
                        })
                    },
                    None => Ok(String::from("请选择文件"))
                }
            }).map_err(|e: error::BlockingError<String>| {
                error::ErrorInternalServerError(e)
            })
            .map(|r|{
                HttpResponse::Ok().body(r)
            })
        })
}

//读取配置
fn read_conf(field:&str) -> Config{
    let mut conf = Config::default();
    let field = Some(field);
    if let Ok(c) = ini::Ini::load_from_file(CONF_FILE_NAME) {
        c.get_from(field, "port").map(|port|{
            conf.port = port.to_string();
        });
        conf.path = c.get_from(field, "path").unwrap_or(std::env::current_dir().unwrap().as_os_str().to_str().unwrap()).to_string();
        c.get_from(field, "max_file_size").map(|max_file_size|{
            conf.max_file_size = max_file_size.parse::<usize>()
            .and_then(|s|{
                Ok(s * 1024 * 1024) //MB
            })
            .unwrap_or(DEFAULT_MAX_FILE_SIZE);
        });
        c.get_from(field, "pwd").map(|pwd|{
            conf.pwd = pwd.to_string();
        });
        c.get_from(field, "uri").map(|uri|{
            conf.uri = uri.to_string();
        });
    };
    println!("{:?}", conf);
    conf
}

fn index(state: web::Data<AppState>) -> HttpResponse {
    HttpResponse::Ok().body(state.index.clone())
}

fn main() -> std::io::Result<()> {
    std::env::set_var("RUST_LOG", "actix_server=error,actix_web=error");
    env_logger::init();

    let conf = read_conf("release");
    let uri = conf.uri.clone();
    let port = conf.port.clone();

    let mut context = Context::new();
    context.insert("path", &conf.path.clone());
    context.insert("uri", &conf.uri.clone());
    context.insert("max_file_size", &(((conf.max_file_size as f32/1024.0/1024.0)*10.0).round()/10.0));

    // A one off template
    Tera::one_off("index", &Context::new(), true).unwrap();

    let rendered = TEMPLATES.render("index.html", &context).expect("模板渲染失败");

    let data = AppState{
        conf,
        index: rendered
    };

    HttpServer::new(move || {
        App::new()
            .data(data.clone())
            .wrap(middleware::Logger::default())
            .service(
                web::resource(&uri.clone()).route(web::get().to(index)),
            ).service(
                web::resource(&format!("{}/upload", uri.clone())).route(web::post().to_async(upload)),
            )
    })
    .bind(&format!("0.0.0.0:{}", port))?
    .run()
}