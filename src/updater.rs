use crate::{common::do_check_software_update, hbbs_http::create_http_client_with_url};
use hbb_common::{bail, config, log, ResultType};
use std::{
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{channel, Receiver, Sender},
        Mutex,
    },
    time::{Duration, Instant},
};

enum UpdateMsg {
    CheckUpdate,
    Exit,
}

lazy_static::lazy_static! {
    static ref TX_MSG : Mutex<Sender<UpdateMsg>> = Mutex::new(start_auto_update_check());
}

static CONTROLLING_SESSION_COUNT: AtomicUsize = AtomicUsize::new(0);

const DUR_ONE_DAY: Duration = Duration::from_secs(60 * 60 * 24);

fn parse_total_size_from_headers(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|content_range| content_range.to_str().ok())
        .and_then(|content_range| content_range.rsplit('/').next())
        .and_then(|total_size| total_size.trim().parse::<u64>().ok())
        .or_else(|| {
            headers
                .get(reqwest::header::CONTENT_LENGTH)
                .and_then(|content_length| content_length.to_str().ok())
                .and_then(|content_length| content_length.parse::<u64>().ok())
        })
}

fn get_remote_file_size(client: &reqwest::blocking::Client, download_url: &str) -> ResultType<u64> {
    match client.head(download_url).send() {
        Ok(response) if response.status().is_success() => {
            if let Some(total_size) = parse_total_size_from_headers(response.headers()) {
                return Ok(total_size);
            }
            log::warn!(
                "HEAD {} succeeded but did not expose a usable file size, falling back to ranged GET",
                download_url
            );
        }
        Ok(response) => {
            log::warn!(
                "HEAD {} returned {}, falling back to ranged GET for file size",
                download_url,
                response.status()
            );
        }
        Err(err) => {
            log::warn!(
                "HEAD {} failed while probing file size: {}, falling back to ranged GET",
                download_url,
                err
            );
        }
    }

    let response = client
        .get(download_url)
        .header(reqwest::header::RANGE, "bytes=0-0")
        .send()?;
    if !response.status().is_success() {
        bail!("Failed to get the file size: {}", response.status());
    }
    let Some(total_size) = parse_total_size_from_headers(response.headers()) else {
        bail!("Failed to get content length");
    };
    Ok(total_size)
}

#[cfg(target_os = "windows")]
fn prefer_msi_installer() -> bool {
    if crate::common::is_custom_client() {
        return false;
    }
    match crate::platform::is_msi_installed() {
        Ok(installed) => installed,
        Err(e) => {
            log::warn!(
                "Failed to detect MSI install mode, fallback to exe updater: {}",
                e
            );
            false
        }
    }
}

pub fn update_controlling_session_count(count: usize) {
    CONTROLLING_SESSION_COUNT.store(count, Ordering::SeqCst);
}

#[allow(dead_code)]
pub fn start_auto_update() {
    let _sender = TX_MSG.lock().unwrap();
}

#[allow(dead_code)]
pub fn manually_check_update() -> ResultType<()> {
    let sender = TX_MSG.lock().unwrap();
    sender.send(UpdateMsg::CheckUpdate)?;
    Ok(())
}

#[allow(dead_code)]
pub fn stop_auto_update() {
    let sender = TX_MSG.lock().unwrap();
    sender.send(UpdateMsg::Exit).unwrap_or_default();
}

#[inline]
fn has_no_active_conns() -> bool {
    let conns = crate::Connection::alive_conns();
    conns.is_empty() && has_no_controlling_conns()
}

#[cfg(any(not(target_os = "windows"), feature = "flutter"))]
fn has_no_controlling_conns() -> bool {
    CONTROLLING_SESSION_COUNT.load(Ordering::SeqCst) == 0
}

#[cfg(not(any(not(target_os = "windows"), feature = "flutter")))]
fn has_no_controlling_conns() -> bool {
    let app_exe = format!("{}.exe", crate::get_app_name().to_lowercase());
    for arg in [
        "--connect",
        "--play",
        "--file-transfer",
        "--view-camera",
        "--port-forward",
        "--rdp",
    ] {
        if !crate::platform::get_pids_of_process_with_first_arg(&app_exe, arg).is_empty() {
            return false;
        }
    }
    true
}

fn start_auto_update_check() -> Sender<UpdateMsg> {
    let (tx, rx) = channel();
    std::thread::spawn(move || start_auto_update_check_(rx));
    return tx;
}

fn start_auto_update_check_(rx_msg: Receiver<UpdateMsg>) {
    std::thread::sleep(Duration::from_secs(30));
    if let Err(e) = check_update(false) {
        log::error!("Error checking for updates: {}", e);
    }

    const MIN_INTERVAL: Duration = Duration::from_secs(60 * 10);
    const RETRY_INTERVAL: Duration = Duration::from_secs(60 * 30);
    let mut last_check_time = Instant::now();
    let mut check_interval = DUR_ONE_DAY;
    loop {
        let recv_res = rx_msg.recv_timeout(check_interval);
        match &recv_res {
            Ok(UpdateMsg::CheckUpdate) | Err(_) => {
                if last_check_time.elapsed() < MIN_INTERVAL {
                    // log::debug!("Update check skipped due to minimum interval.");
                    continue;
                }
                // Don't check update if there are alive connections.
                if !has_no_active_conns() {
                    check_interval = RETRY_INTERVAL;
                    continue;
                }
                if let Err(e) = check_update(matches!(recv_res, Ok(UpdateMsg::CheckUpdate))) {
                    log::error!("Error checking for updates: {}", e);
                    check_interval = RETRY_INTERVAL;
                } else {
                    last_check_time = Instant::now();
                    check_interval = DUR_ONE_DAY;
                }
            }
            Ok(UpdateMsg::Exit) => break,
        }
    }
}

fn check_update(manually: bool) -> ResultType<()> {
    #[cfg(target_os = "windows")]
    let update_msi = prefer_msi_installer();
    if !(manually || config::Config::get_bool_option(config::keys::OPTION_ALLOW_AUTO_UPDATE)) {
        return Ok(());
    }
    if do_check_software_update(manually).is_err() {
        // ignore
        return Ok(());
    }

    let update_url = crate::common::SOFTWARE_UPDATE_URL.lock().unwrap().clone();
    if update_url.is_empty() {
        log::debug!("No update available.");
    } else {
        let (version, download_url) = resolve_download_url(&update_url)?;
        log::debug!("New version available: {}", &version);
        let client = create_http_client_with_url(&download_url);
        let Some(file_path) = get_download_file_from_url(&download_url) else {
            bail!("Failed to get the file path from the URL: {}", download_url);
        };
        let mut is_file_exists = false;
        if file_path.exists() {
            // Check if the file size is the same as the server file size
            // If the file size is the same, we don't need to download it again.
            let file_size = std::fs::metadata(&file_path)?.len();
            let total_size = get_remote_file_size(&client, &download_url)?;
            if file_size == total_size {
                is_file_exists = true;
            } else {
                std::fs::remove_file(&file_path)?;
            }
        }
        if !is_file_exists {
            let response = client.get(&download_url).send()?;
            if !response.status().is_success() {
                bail!(
                    "Failed to download the new version file: {}",
                    response.status()
                );
            }
            let file_data = response.bytes()?;
            let mut file = std::fs::File::create(&file_path)?;
            file.write_all(&file_data)?;
        }
        // We have checked if the `conns` is empty before, but we need to check again.
        // No need to care about the downloaded file here, because it's rare case that the `conns` are empty
        // before the download, but not empty after the download.
        if has_no_active_conns() {
            #[cfg(target_os = "windows")]
            update_new_version(update_msi, &version, &file_path);
        }
    }
    Ok(())
}

fn resolve_download_url(update_url: &str) -> ResultType<(String, String)> {
    let Some(version) = get_version_from_update_url(update_url) else {
        bail!("Unsupported update url: {}", update_url);
    };

    if is_release_page_url(update_url) {
        let download_root = get_release_download_root(update_url)?;
        let download_file = get_release_asset_filename(&version)?;
        Ok((version, format!("{download_root}/{download_file}")))
    } else {
        Ok((version, update_url.to_owned()))
    }
}

fn is_release_page_url(update_url: &str) -> bool {
    update_url.contains("/releases/tag/")
}

fn get_version_from_update_url(update_url: &str) -> Option<String> {
    let prefix = format!("{}-", crate::common::OFFICIAL_RELEASE_ASSET_PREFIX);
    if let Some(start) = update_url.find(&prefix) {
        let rest = &update_url[start + prefix.len()..];
        for end_marker in ["-x86_64.", "-x86-sciter.", "-aarch64."] {
            if let Some(end) = rest.find(end_marker) {
                return Some(rest[..end].to_owned());
            }
        }
    }

    update_url
        .split('?')
        .next()
        .and_then(|url_without_query| url_without_query.rsplit('/').next())
        .filter(|segment| !segment.is_empty())
        .map(ToOwned::to_owned)
}

fn get_release_download_root(release_page_url: &str) -> ResultType<String> {
    let download_root = release_page_url.replacen("/releases/tag/", "/releases/download/", 1);
    if download_root == release_page_url {
        bail!("Unsupported release page url: {}", release_page_url);
    }
    Ok(download_root)
}

pub fn get_release_asset_filename(version: &str) -> ResultType<String> {
    let prefix = crate::common::OFFICIAL_RELEASE_ASSET_PREFIX;

    #[cfg(target_os = "windows")]
    {
        if cfg!(feature = "flutter") {
            let extension = if prefer_msi_installer() { "msi" } else { "exe" };
            return Ok(format!("{prefix}-{version}-x86_64.{extension}"));
        }

        return Ok(format!("{prefix}-{version}-x86-sciter.exe"));
    }

    #[cfg(target_os = "macos")]
    {
        return if cfg!(target_arch = "x86_64") {
            Ok(format!("{prefix}-{version}-x86_64.dmg"))
        } else if cfg!(target_arch = "aarch64") {
            Ok(format!("{prefix}-{version}-aarch64.dmg"))
        } else {
            bail!("unsupported")
        };
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        bail!("unsupported")
    }
}

#[cfg(target_os = "windows")]
fn update_new_version(update_msi: bool, version: &str, file_path: &PathBuf) {
    log::debug!(
        "New version is downloaded, update begin, update msi: {update_msi}, version: {version}, file: {:?}",
        file_path.to_str()
    );
    if let Some(p) = file_path.to_str() {
        if let Some(session_id) = crate::platform::get_current_process_session_id() {
            if update_msi {
                match crate::platform::update_me_msi(p, true) {
                    Ok(_) => {
                        log::debug!("New version \"{}\" updated.", version);
                    }
                    Err(e) => {
                        log::error!(
                            "Failed to install the new msi version  \"{}\": {}",
                            version,
                            e
                        );
                        std::fs::remove_file(&file_path).ok();
                    }
                }
            } else {
                let custom_client_staging_dir = if crate::is_custom_client() {
                    let custom_client_staging_dir =
                        crate::platform::get_custom_client_staging_dir();
                    if let Err(e) = crate::platform::handle_custom_client_staging_dir_before_update(
                        &custom_client_staging_dir,
                    ) {
                        log::error!(
                            "Failed to handle custom client staging dir before update: {}",
                            e
                        );
                        std::fs::remove_file(&file_path).ok();
                        return;
                    }
                    Some(custom_client_staging_dir)
                } else {
                    // Clean up any residual staging directory from previous custom client
                    let staging_dir = crate::platform::get_custom_client_staging_dir();
                    hbb_common::allow_err!(crate::platform::remove_custom_client_staging_dir(
                        &staging_dir
                    ));
                    None
                };
                let update_launched = match crate::platform::launch_privileged_process(
                    session_id,
                    &format!("{} --update", p),
                ) {
                    Ok(h) => {
                        if h.is_null() {
                            log::error!("Failed to update to the new version: {}", version);
                            false
                        } else {
                            log::debug!("New version \"{}\" is launched.", version);
                            true
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to run the new version: {}", e);
                        false
                    }
                };
                if !update_launched {
                    if let Some(dir) = custom_client_staging_dir {
                        hbb_common::allow_err!(crate::platform::remove_custom_client_staging_dir(
                            &dir
                        ));
                    }
                    std::fs::remove_file(&file_path).ok();
                }
            }
        } else {
            log::error!(
                "Failed to get the current process session id, Error {}",
                std::io::Error::last_os_error()
            );
            std::fs::remove_file(&file_path).ok();
        }
    } else {
        // unreachable!()
        log::error!(
            "Failed to convert the file path to string: {}",
            file_path.display()
        );
    }
}

pub fn get_download_file_from_url(url: &str) -> Option<PathBuf> {
    let mut filename = if let Ok(parsed) = url::Url::parse(url) {
        parsed.path_segments()?.next_back()?.to_owned()
    } else {
        url.split('/').next_back()?.to_owned()
    };

    if let Some(decoded_tail) = filename.rsplit("%2F").next() {
        filename = decoded_tail.to_owned();
    }
    if let Some(decoded_tail) = filename.rsplit("%2f").next() {
        filename = decoded_tail.to_owned();
    }
    if let Some(clean_filename) = filename.split('?').next() {
        filename = clean_filename.to_owned();
    }
    if filename.is_empty() {
        return None;
    }

    Some(std::env::temp_dir().join(filename))
}
