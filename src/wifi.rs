// pub async fn connect_wifi(controller: &mut WifiController<'static>) -> Result<(), &'static str> {
//     println!("Starting wifi connection...");

//     controller.set_network(ssid, password).unwrap();

//     println!("Waiting for connection...");
//     loop {
//         if let WifiState::Connected(_) = controller.get_status() {
//             break;
//         }
//         Timer::after(Duration::from_millis(100)).await;
//     }

//     Ok(())
// }

// let timg0 = TimerGroup::new(peripherals.TIMG0);
//     let mut rng = Rng::new(peripherals.RNG);

//     let esp_wifi_ctrl = &*mk_static!(
//         EspWifiController<'static>,
//         init(timg0.timer0, rng.clone(), peripherals.RADIO_CLK).unwrap()
//     );

//     let (controller, interfaces) = esp_wifi::wifi::new(&esp_wifi_ctrl, peripherals.WIFI).unwrap();

//     let wifi_interface = interfaces.sta;

//     cfg_if::cfg_if! {
//         if #[cfg(feature = "esp32")] {
//             let timg1 = TimerGroup::new(peripherals.TIMG1);
//             esp_hal_embassy::init(timg1.timer0);
//         } else {
//             use esp_hal::timer::systimer::SystemTimer;
//             let systimer = SystemTimer::new(peripherals.SYSTIMER);
//             esp_hal_embassy::init(systimer.alarm0);
//         }
//     }

//     let config = embassy_net::Config::dhcpv4(Default::default());

//     let seed = (rng.random() as u64) << 32 | rng.random() as u64;

//     // Initialize WiFi
//     let mut wifi = initialize(EspWifiInitFor::Wifi).unwrap();
//     let mut controller = wifi.start().unwrap();

//     // Connect to WiFi
//     match connect_wifi(&mut controller).await {
//         Ok(_) => info!("WiFi connected!"),
//         Err(e) => {
//             warn!("Failed to connect to WiFi: {:?}", e);
//             return;
//         }
//     }
