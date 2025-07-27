use core::fmt::Write;
use defmt::info;
use embassy_net::{
    dns::DnsSocket,
    tcp::client::{TcpClient, TcpClientState},
    Runner, StackResources,
};
use embassy_time::{Duration, Timer};

use esp_wifi::wifi::{
    ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiState,
};

use heapless::{String, Vec};
use panic_rtt_target as _;
use reqwless::client::HttpClient;

const SSID: &str = env!("ESP_WIFI_SSID");
const PASSWORD: &str = env!("ESP_WIFI_PASSWORD");

#[embassy_executor::task]
pub async fn connection(mut controller: WifiController<'static>) {
    info!("start connection task");
    // info!("Device capabilities: {:?}", controller.capabilities());
    loop {
        match esp_wifi::wifi::wifi_state() {
            WifiState::StaConnected => {
                // wait until we're no longer connected
                controller.wait_for_event(WifiEvent::StaDisconnected).await;
                Timer::after(Duration::from_millis(5000)).await
            }
            _ => {}
        }
        if !matches!(controller.is_started(), Ok(true)) {
            let client_config = Configuration::Client(ClientConfiguration {
                ssid: SSID.into(),
                password: PASSWORD.into(),
                ..Default::default()
            });
            controller.set_configuration(&client_config).unwrap();
            info!("Starting wifi");
            controller.start_async().await.unwrap();
            info!("Wifi started!");
        }
        info!("About to connect...");

        match controller.connect_async().await {
            Ok(_) => info!("Wifi connected!"),
            Err(e) => {
                info!("Failed to connect to wifi $r");
                Timer::after(Duration::from_millis(5000)).await
            }
        }
    }
}

#[embassy_executor::task]
pub async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}
