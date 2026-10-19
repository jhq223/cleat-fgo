//! Android package metadata and download notifications, implemented through JNI.
use cleat::android::{
    jni::{
        self, Env, JValue, jni_sig, jni_str,
        objects::{JObject, JString},
    },
    with_java,
};
use std::path::Path;

fn context<'a>(env: &mut Env<'a>) -> jni::errors::Result<JObject<'a>> {
    env.call_static_method(
        jni_str!("android/app/ActivityThread"),
        jni_str!("currentApplication"),
        jni_sig!("()Landroid/app/Application;"),
        &[],
    )?
    .l()
}

pub fn version_code() -> Result<i64, String> {
    with_java(|env| {
        let app = context(env)?;
        let package = env
            .call_method(
                &app,
                jni_str!("getPackageName"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        let manager = env
            .call_method(
                &app,
                jni_str!("getPackageManager"),
                jni_sig!("()Landroid/content/pm/PackageManager;"),
                &[],
            )?
            .l()?;
        let info = env
            .call_method(
                &manager,
                jni_str!("getPackageInfo"),
                jni_sig!("(Ljava/lang/String;I)Landroid/content/pm/PackageInfo;"),
                &[JValue::Object(&package), JValue::Int(0)],
            )?
            .l()?;
        Ok(i64::from(
            env.get_field(&info, jni_str!("versionCode"), jni_sig!("I"))?
                .i()?,
        ))
    })
    .map_err(|e| e.to_string())
}

pub fn validate_apk(path: &Path, expected_code: i64) -> Result<(), String> {
    let (package, code) = with_java(|env| {
        let app = context(env)?;
        let manager = env
            .call_method(
                &app,
                jni_str!("getPackageManager"),
                jni_sig!("()Landroid/content/pm/PackageManager;"),
                &[],
            )?
            .l()?;
        let path = env.new_string(path.to_string_lossy())?;
        let info = env
            .call_method(
                &manager,
                jni_str!("getPackageArchiveInfo"),
                jni_sig!("(Ljava/lang/String;I)Landroid/content/pm/PackageInfo;"),
                &[JValue::Object(&path), JValue::Int(0)],
            )?
            .l()?;
        let package = env
            .get_field(
                &info,
                jni_str!("packageName"),
                jni_sig!("Ljava/lang/String;"),
            )?
            .l()?;
        let package = JString::cast_local(env, package)?.try_to_string(env)?;
        let code = i64::from(
            env.get_field(&info, jni_str!("versionCode"), jni_sig!("I"))?
                .i()?,
        );
        Ok((package, code))
    })
    .map_err(|e| e.to_string())?;
    if package != "com.aniplex.fategrandorder" || code != expected_code {
        return Err("downloaded APK package/version differs from its manifest".into());
    }
    Ok(())
}

/// Register a verified local file with Android's Downloads UI. Android retains
/// control of the installation prompt and signature checks; no silent install.
pub fn notify_apk(path: &Path, size: u64) -> Result<(), String> {
    with_java(|env| {
        let app = context(env)?;
        let service = env.new_string("download")?;
        let manager = env
            .call_method(
                &app,
                jni_str!("getSystemService"),
                jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                &[JValue::Object(&service)],
            )?
            .l()?;
        let title = env.new_string("FGO 更新已下载")?;
        let description = env.new_string("点击后由系统确认安装")?;
        let mime = env.new_string("application/vnd.android.package-archive")?;
        let path = env.new_string(path.to_string_lossy())?;
        env.call_method(
            &manager,
            jni_str!("addCompletedDownload"),
            jni_sig!(
                "(Ljava/lang/String;Ljava/lang/String;ZLjava/lang/String;Ljava/lang/String;JZ)J"
            ),
            &[
                JValue::Object(&title),
                JValue::Object(&description),
                JValue::Bool(false),
                JValue::Object(&mime),
                JValue::Object(&path),
                JValue::Long(size as i64),
                JValue::Bool(true),
            ],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}
