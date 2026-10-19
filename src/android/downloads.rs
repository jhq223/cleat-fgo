//! Publish the Chaldea response in Android's shared Downloads collection.
use super::{context, with_java};
use cleat::android::jni::{
    self, Env, JValue, jni_sig, jni_str,
    objects::{JObject, JString},
};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    os::fd::FromRawFd,
};

const DIRECTORY: &str = "Download/cleat-fgo/";
const NAME: &str = "chaldea-login.json";
const MAX_EXPORT: u64 = 32 * 1024 * 1024;

pub fn export_login(body: &[u8]) -> Result<bool, String> {
    let sdk = with_java(|env| {
        env.get_static_field(
            jni_str!("android/os/Build$VERSION"),
            jni_str!("SDK_INT"),
            jni_sig!("I"),
        )?
        .i()
    })
    .map_err(|error| error.to_string())?;
    if sdk < 29 {
        return Ok(false);
    }
    let (uri, created) = find_or_create()?;
    let result = write_response(&uri, body);
    if result.is_err() && created {
        let _ = delete(&uri);
    }
    result.map(|()| true)
}

fn resolver<'a>(env: &mut Env<'a>) -> jni::errors::Result<JObject<'a>> {
    let app = context(env)?;
    env.call_method(
        &app,
        jni_str!("getContentResolver"),
        jni_sig!("()Landroid/content/ContentResolver;"),
        &[],
    )?
    .l()
}

fn uri<'a>(env: &mut Env<'a>, value: &str) -> jni::errors::Result<JObject<'a>> {
    let value = env.new_string(value)?;
    env.call_static_method(
        jni_str!("android/net/Uri"),
        jni_str!("parse"),
        jni_sig!("(Ljava/lang/String;)Landroid/net/Uri;"),
        &[JValue::Object(&value)],
    )?
    .l()
}

fn values<'a>(env: &mut Env<'a>) -> jni::errors::Result<JObject<'a>> {
    env.new_object(
        jni_str!("android/content/ContentValues"),
        jni_sig!("()V"),
        &[],
    )
}

fn put(env: &mut Env, values: &JObject, key: &str, value: &str) -> jni::errors::Result<()> {
    let key = env.new_string(key)?;
    let value = env.new_string(value)?;
    env.call_method(
        values,
        jni_str!("put"),
        jni_sig!("(Ljava/lang/String;Ljava/lang/String;)V"),
        &[JValue::Object(&key), JValue::Object(&value)],
    )?;
    Ok(())
}

fn find_or_create() -> Result<(String, bool), String> {
    with_java(|env| {
        let resolver = resolver(env)?;
        let volume = env.new_string("external_primary")?;
        let collection = env.call_static_method(
            jni_str!("android/provider/MediaStore$Downloads"),
            jni_str!("getContentUri"),
            jni_sig!("(Ljava/lang/String;)Landroid/net/Uri;"),
            &[JValue::Object(&volume)],
        )?.l()?;
        let query_uri = env.call_static_method(
            jni_str!("android/provider/MediaStore"),
            jni_str!("setIncludePending"),
            jni_sig!("(Landroid/net/Uri;)Landroid/net/Uri;"),
            &[JValue::Object(&collection)],
        )?.l()?;
        let projection = env.new_object_array(1, jni_str!("java/lang/String"), JObject::null())?;
        let column = env.new_string("_id")?;
        projection.set_element(env, 0, &column)?;
        let selection = env.new_string("relative_path = ? AND _display_name = ? AND owner_package_name = ?")?;
        let args = env.new_object_array(3, jni_str!("java/lang/String"), JObject::null())?;
        for (index, value) in [DIRECTORY, NAME].iter().enumerate() {
            let value = env.new_string(value)?;
            args.set_element(env, index, &value)?;
        }
        let app = context(env)?;
        let package = env.call_method(&app, jni_str!("getPackageName"), jni_sig!("()Ljava/lang/String;"), &[])?.l()?;
        args.set_element(env, 2, &package)?;
        let cursor = env.call_method(
            &resolver,
            jni_str!("query"),
            jni_sig!("(Landroid/net/Uri;[Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;Ljava/lang/String;)Landroid/database/Cursor;"),
            &[JValue::Object(&query_uri), JValue::Object(&projection), JValue::Object(&selection), JValue::Object(&args), JValue::Object(&JObject::null())],
        )?.l()?;
        if cursor.is_null() {
            return Err(jni::errors::Error::NullPtr("MediaStore query"));
        }
        let found = (|| {
            if env.call_method(&cursor, jni_str!("moveToFirst"), jni_sig!("()Z"), &[])?.z()? {
                env.call_method(&cursor, jni_str!("getLong"), jni_sig!("(I)J"), &[JValue::Int(0)])?.j().map(Some)
            } else {
                Ok(None)
            }
        })();
        let closed = env.call_method(&cursor, jni_str!("close"), jni_sig!("()V"), &[]);
        let found = found?;
        closed?;
        let created = found.is_none();
        let result = if let Some(id) = found {
            env.call_static_method(
                jni_str!("android/content/ContentUris"), jni_str!("withAppendedId"),
                jni_sig!("(Landroid/net/Uri;J)Landroid/net/Uri;"),
                &[JValue::Object(&collection), JValue::Long(id)],
            )?.l()?
        } else {
            let values = values(env)?;
            put(env, &values, "_display_name", NAME)?;
            put(env, &values, "mime_type", "application/json")?;
            put(env, &values, "relative_path", DIRECTORY)?;
            put(env, &values, "is_pending", "1")?;
            env.call_method(
                &resolver, jni_str!("insert"),
                jni_sig!("(Landroid/net/Uri;Landroid/content/ContentValues;)Landroid/net/Uri;"),
                &[JValue::Object(&collection), JValue::Object(&values)],
            )?.l()?
        };
        if result.is_null() {
            return Err(jni::errors::Error::NullPtr("MediaStore insert"));
        }
        let text = env.call_method(&result, jni_str!("toString"), jni_sig!("()Ljava/lang/String;"), &[])?.l()?;
        Ok((JString::cast_local(env, text)?.try_to_string(env)?, created))
    }).map_err(|error| error.to_string())
}

fn open(value: &str) -> Result<File, String> {
    let fd = with_java(|env| {
        let resolver = resolver(env)?;
        let uri = uri(env, value)?;
        let mode = env.new_string("rw")?;
        let descriptor = env
            .call_method(
                &resolver,
                jni_str!("openFileDescriptor"),
                jni_sig!("(Landroid/net/Uri;Ljava/lang/String;)Landroid/os/ParcelFileDescriptor;"),
                &[JValue::Object(&uri), JValue::Object(&mode)],
            )?
            .l()?;
        if descriptor.is_null() {
            return Err(jni::errors::Error::NullPtr("download file descriptor"));
        }
        env.call_method(&descriptor, jni_str!("detachFd"), jni_sig!("()I"), &[])?
            .i()
    })
    .map_err(|error| error.to_string())?;
    if fd < 0 {
        return Err("MediaStore returned an invalid file descriptor".into());
    }
    // detachFd transfers ownership from ParcelFileDescriptor to this File.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn pending(value: &str, pending: bool) -> Result<(), String> {
    let count = with_java(|env| {
        let resolver = resolver(env)?;
        let uri = uri(env, value)?;
        let values = values(env)?;
        put(env, &values, "is_pending", if pending { "1" } else { "0" })?;
        env.call_method(
            &resolver, jni_str!("update"),
            jni_sig!("(Landroid/net/Uri;Landroid/content/ContentValues;Ljava/lang/String;[Ljava/lang/String;)I"),
            &[JValue::Object(&uri), JValue::Object(&values), JValue::Object(&JObject::null()), JValue::Object(&JObject::null())],
        )?.i()
    }).map_err(|error| error.to_string())?;
    if count != 1 {
        return Err("download entry no longer exists".into());
    }
    Ok(())
}

fn delete(value: &str) -> Result<(), String> {
    with_java(|env| {
        let resolver = resolver(env)?;
        let uri = uri(env, value)?;
        env.call_method(
            &resolver,
            jni_str!("delete"),
            jni_sig!("(Landroid/net/Uri;Ljava/lang/String;[Ljava/lang/String;)I"),
            &[
                JValue::Object(&uri),
                JValue::Object(&JObject::null()),
                JValue::Object(&JObject::null()),
            ],
        )?;
        Ok(())
    })
    .map_err(|error| error.to_string())
}

fn replace(file: &mut File, bytes: &[u8]) -> std::io::Result<()> {
    file.seek(SeekFrom::Start(0))?;
    file.write_all(bytes)?;
    file.set_len(bytes.len() as u64)?;
    file.sync_all()
}

fn read_previous(uri: &str) -> Result<Vec<u8>, String> {
    let mut file = open(uri)?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_EXPORT {
        return Err("existing download exceeds export size limit".into());
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

fn write_file(uri: &str, bytes: &[u8]) -> Result<(), String> {
    let mut file = open(uri)?;
    replace(&mut file, bytes).map_err(|error| error.to_string())
}

fn write_response(uri: &str, body: &[u8]) -> Result<(), String> {
    let previous = read_previous(uri)?;
    // MediaStore may rename pending files. Close every descriptor before changing
    // metadata so shared-storage providers can complete their rename/unlink work.
    pending(uri, true)?;
    let result = write_file(uri, body).and_then(|()| pending(uri, false));
    if result.is_err() && write_file(uri, &previous).is_ok() {
        let _ = pending(uri, false);
    }
    result
}
