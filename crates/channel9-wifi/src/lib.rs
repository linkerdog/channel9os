use core::convert::TryInto;

use anyhow::{Context, Result};
use channel9_core::{WifiConfig, WifiCredential};
use embedded_svc::wifi::{AuthMethod, ClientConfiguration, Configuration};
use esp_idf_hal::modem::WifiModem;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sys::{esp, esp_wifi_set_ps};
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiNetwork {
    pub ssid: String,
    pub channel: u8,
    pub signal_dbm: i8,
    pub auth_method: Option<AuthMethod>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiConnectionInfo {
    pub ssid: String,
    pub ip: String,
    pub gateway: String,
    pub dns_primary: String,
    pub dns_secondary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiStatus {
    Disabled,
    Idle,
    Started,
    Connected,
    Failed,
}

pub struct Channel9Wifi {
    wifi: BlockingWifi<EspWifi<'static>>,
    status: WifiStatus,
    last_ssid: Option<String>,
    last_error: Option<String>,
}

#[derive(Debug, Clone)]
struct WifiConnectTarget {
    credential: WifiCredential,
    auth_method: Option<AuthMethod>,
    channel: Option<u8>,
}

impl WifiConnectTarget {
    fn without_hint(credential: WifiCredential) -> Self {
        Self {
            credential,
            auth_method: None,
            channel: None,
        }
    }
}

impl Channel9Wifi {
    pub fn new(modem: WifiModem<'static>) -> Result<Self> {
        let sys_loop = EspSystemEventLoop::take()?;
        let nvs = EspDefaultNvsPartition::take()?;
        let wifi = BlockingWifi::wrap(EspWifi::new(modem, sys_loop.clone(), Some(nvs))?, sys_loop)?;

        Ok(Self {
            wifi,
            status: WifiStatus::Idle,
            last_ssid: None,
            last_error: None,
        })
    }

    pub fn status(&self) -> WifiStatus {
        self.status
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn last_ssid(&self) -> Option<&str> {
        self.last_ssid.as_deref()
    }

    pub fn connection_info(&self) -> Option<WifiConnectionInfo> {
        if self.status != WifiStatus::Connected {
            return None;
        }

        let ip_info = self.wifi.wifi().sta_netif().get_ip_info().ok()?;
        let dns_primary = self.wifi.wifi().sta_netif().get_dns();
        let dns_secondary = self.wifi.wifi().sta_netif().get_secondary_dns();
        Some(WifiConnectionInfo {
            ssid: self
                .last_ssid
                .clone()
                .unwrap_or_else(|| "unknown".to_owned()),
            ip: ip_info.ip.to_string(),
            gateway: ip_info.subnet.gateway.to_string(),
            dns_primary: dns_primary.to_string(),
            dns_secondary: dns_secondary.to_string(),
        })
    }

    pub fn scan(&mut self) -> Result<Vec<WifiNetwork>> {
        if !self.wifi.is_started()? {
            self.wifi.start()?;
            disable_wifi_modem_sleep()?;
            self.status = WifiStatus::Started;
        }

        let mut networks: Vec<WifiNetwork> = self
            .wifi
            .scan()?
            .into_iter()
            .map(|ap| WifiNetwork {
                ssid: ap.ssid.as_str().to_owned(),
                channel: ap.channel,
                signal_dbm: ap.signal_strength,
                auth_method: ap.auth_method,
            })
            .collect();
        networks.sort_by(|left, right| right.signal_dbm.cmp(&left.signal_dbm));
        Ok(networks)
    }

    pub fn connect_first_saved(&mut self, config: &WifiConfig) -> Result<Option<String>> {
        if !config.connect_at_startup {
            self.status = WifiStatus::Disabled;
            return Ok(None);
        }

        if config.credentials.is_empty() {
            self.status = WifiStatus::Idle;
            self.last_ssid = None;
            self.last_error = None;
            return Ok(None);
        };

        let candidates = match self.saved_credentials_in_scan_order(config) {
            Ok(candidates) if !candidates.is_empty() => candidates,
            Ok(_) => return Ok(None),
            Err(err) => {
                log::warn!(
                    "wifi startup scan failed; trying saved networks in config order: {err:?}"
                );
                config
                    .credentials
                    .iter()
                    .cloned()
                    .map(WifiConnectTarget::without_hint)
                    .collect()
            }
        };

        self.connect_candidates(&candidates)
    }

    pub fn stop(&mut self) -> Result<()> {
        if self.wifi.is_connected()? {
            self.wifi
                .disconnect()
                .context("failed to disconnect wifi")?;
        }
        if self.wifi.is_started()? {
            self.wifi.stop().context("failed to stop wifi")?;
        }
        self.status = WifiStatus::Idle;
        Ok(())
    }

    pub fn connect(&mut self, credential: &WifiCredential) -> Result<()> {
        self.connect_with_hint(credential, None, None)
    }

    pub fn connect_saved(&mut self, credential: &WifiCredential) -> Result<()> {
        match self.scan_matching_network(&credential.ssid) {
            Ok(Some(network)) => self.connect_network(credential, &network),
            Ok(None) => self.connect(credential),
            Err(err) => {
                log::warn!(
                    "wifi saved scan failed for {}; trying saved credential: {err:?}",
                    credential.ssid
                );
                self.connect(credential)
            }
        }
    }

    pub fn connect_network(
        &mut self,
        credential: &WifiCredential,
        network: &WifiNetwork,
    ) -> Result<()> {
        self.connect_with_hint(credential, network.auth_method, Some(network.channel))
    }

    fn connect_with_hint(
        &mut self,
        credential: &WifiCredential,
        auth_method: Option<AuthMethod>,
        channel: Option<u8>,
    ) -> Result<()> {
        self.last_ssid = Some(credential.ssid.clone());
        self.last_error = None;

        let configuration = Configuration::Client(ClientConfiguration {
            ssid: credential
                .ssid
                .as_str()
                .try_into()
                .context("wifi ssid is too long")?,
            bssid: None,
            auth_method: select_auth_method(credential, auth_method),
            password: credential
                .password
                .as_str()
                .try_into()
                .context("wifi password is too long")?,
            channel,
            ..Default::default()
        });

        self.wifi.set_configuration(&configuration)?;
        if !self.wifi.is_started()? {
            self.wifi.start()?;
        }
        disable_wifi_modem_sleep()?;
        self.status = WifiStatus::Started;
        let result = self.wifi.connect().and_then(|_| self.wifi.wait_netif_up());
        match result {
            Ok(()) => {
                self.status = WifiStatus::Connected;
                Ok(())
            }
            Err(err) => {
                self.status = WifiStatus::Failed;
                self.last_error = Some(format!("{err:?}"));
                Err(err.into())
            }
        }
    }

    fn saved_credentials_in_scan_order(
        &mut self,
        config: &WifiConfig,
    ) -> Result<Vec<WifiConnectTarget>> {
        let networks = self.scan()?;
        let mut credentials = Vec::new();
        for network in networks {
            if credentials
                .iter()
                .any(|target: &WifiConnectTarget| target.credential.ssid == network.ssid)
            {
                continue;
            }
            if let Some(credential) = config
                .credentials
                .iter()
                .find(|credential| credential.ssid == network.ssid)
            {
                credentials.push(WifiConnectTarget {
                    credential: credential.clone(),
                    auth_method: network.auth_method,
                    channel: Some(network.channel),
                });
            }
        }
        Ok(credentials)
    }

    fn scan_matching_network(&mut self, ssid: &str) -> Result<Option<WifiNetwork>> {
        Ok(self
            .scan()?
            .into_iter()
            .find(|network| network.ssid == ssid))
    }

    fn connect_candidates(&mut self, credentials: &[WifiConnectTarget]) -> Result<Option<String>> {
        let mut last_error = None;
        for target in credentials {
            match self.connect_with_hint(&target.credential, target.auth_method, target.channel) {
                Ok(()) => return Ok(Some(target.credential.ssid.clone())),
                Err(err) => {
                    log::warn!(
                        "wifi auto connect failed for {}: {err:?}",
                        target.credential.ssid
                    );
                    last_error = Some(err);
                }
            }
        }

        if let Some(err) = last_error {
            Err(err)
        } else {
            Ok(None)
        }
    }
}

fn select_auth_method(credential: &WifiCredential, auth_method: Option<AuthMethod>) -> AuthMethod {
    if credential.password.is_empty() {
        return AuthMethod::None;
    }

    match auth_method {
        Some(AuthMethod::None) | None => AuthMethod::WPA2Personal,
        Some(auth_method) => auth_method,
    }
}

fn disable_wifi_modem_sleep() -> Result<()> {
    esp!(unsafe { esp_wifi_set_ps(0) }).context("failed to disable wifi modem sleep")?;
    Ok(())
}
