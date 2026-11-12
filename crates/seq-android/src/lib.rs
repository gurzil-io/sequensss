//! Android entry point (GameActivity). Compiles to nothing on other platforms.
#![cfg(target_os = "android")]

use sequensss::eframe;
use winit::platform::android::EventLoopBuilderExtAndroid;
use winit::platform::android::activity::AndroidApp;

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("sequensss"),
    );
    std::panic::set_hook(Box::new(|info| log::error!("panic: {info}")));

    // App's external directory (adb push/pull without root), otherwise the internal one.
    let dir = app
        .external_data_path()
        .or_else(|| app.internal_data_path())
        .unwrap_or_else(|| "/data/local/tmp/sequensss".into());
    log::info!("files directory: {}", dir.display());
    // System language (Android Configuration), used if none was chosen in the menu.
    let locale = app.config().language();
    log::info!("system language: {locale:?}");

    let options = eframe::NativeOptions {
        event_loop_builder: Some(Box::new(move |builder| {
            builder.with_android_app(app);
        })),
        ..Default::default()
    };
    if let Err(e) = sequensss::run(options, None, sequensss::Files::android(dir), locale) {
        log::error!("shutdown: {e}");
    }
}
