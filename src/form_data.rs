use std::fs;
use std::io::Write;

use actix_multipart::{Field, MultipartError};
use actix_web::{error, web, Error};
use futures::{future::{err, ok, Either}, Future, Stream};

#[derive(Clone, Debug)]
pub struct FileField{
    pub file_name: String,
    pub temp_file: String,
}

#[derive(Clone, Debug)]
pub struct ParamterField{
    pub value: String
}

#[derive(Clone, Debug)]
pub enum FieldData{
    File(FileField),
    Paramter(ParamterField),
}

#[derive(Debug)]
pub struct FieldInfo{
    pub data: FieldData,
    pub key: String
}

/// Field读取为字符串
pub fn get_string(field: Field, key:String) -> impl Future<Item = FieldInfo, Error = Error> {
    field.fold(vec![], move |mut all_bytes, bytes| {
        all_bytes.extend(bytes);
        ok::<Vec<u8>, MultipartError>(all_bytes)
    })
    .map(|all_bytes|{
        FieldInfo{
            data: FieldData::Paramter(ParamterField{
                value: String::from_utf8(all_bytes).unwrap()
            }),
            key
        }
    })
    .map_err(|e| {
        error::ErrorInternalServerError(format!("字段解析失败:{:?}", e))
    })
}

/// Field保存成文件
pub fn save_file(field: Field, key:String) -> impl Future<Item = FieldInfo, Error = Error> {
    //println!("field={:?}", field);
    let file_name = match field.content_disposition().and_then(|d|{
        d.get_filename().and_then(|name|{
            Some(String::from(name))
        })
    }){
        Some(name) => name,
        _ => return Either::A(err(error::ErrorBadRequest("请上传文件"))),
    };

    let file_path_string = String::from("upload.tmp");
    let file = match fs::File::create(&file_path_string) {
        Ok(file) => file,
        Err(e) => return Either::A(err(error::ErrorInternalServerError(e))),
    };
    
    Either::B(
        field
            .fold((file, file_path_string, 0i64), move |(mut file, path, mut acc), bytes| {
                //线程池中执行保存文件
                web::block(move || {
                    file.write_all(bytes.as_ref()).map_err(|e| {
                        MultipartError::Payload(error::PayloadError::Io(e))
                    })?;
                    acc += bytes.len() as i64;
                    Ok((file, path, acc))
                })
                .map_err(|e: error::BlockingError<MultipartError>| {
                    match e {
                        error::BlockingError::Error(e) => e,
                        error::BlockingError::Canceled => MultipartError::Incomplete,
                    }
                })
            })
            .map(|(_, path, _acc)|{
                FieldInfo{
                    data: FieldData::File(FileField{
                        temp_file:path,
                        file_name: file_name
                    }),
                    key
                }
            })
            .map_err(|e| {
                error::ErrorInternalServerError(e)
            }),
    )
}