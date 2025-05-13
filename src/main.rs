#![no_std]
#![no_main]
mod draw;
mod led;
mod wifi;
use defmt::{info, warn};

use embassy_executor::Spawner;
// use embassy_net::{
//     dns::DnsSocket,
//     tcp::client::{TcpClient, TcpClientState},
// };
use embassy_time::{Duration, Timer};

use esp_hal::{
    clock::CpuClock,
    dma::{DmaRxBuf, DmaTxBuf},
    dma_buffers,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    rmt::Rmt,
    spi::{
        master::{Config, Spi},
        Mode,
    },
    time::Rate,
    timer::{systimer::SystemTimer, timg::TimerGroup},
};

// use esp_wifi::{
//     init,
//     wifi::{ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiState},
//     EspWifiController,
// };

use led::SmartLedsAdapter;
use panic_rtt_target as _;
use reqwless::client::HttpClient;
use smart_leds::{SmartLedsWrite, RGB8};
// use wifi::connect_wifi;
extern crate alloc;
use draw::EPD7in3f;

// macro_rules! mk_static {
//     ($t:ty,$val:expr) => {{
//         static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
//         #[deny(unused_attributes)]
//         let x = STATIC_CELL.uninit().write(($val));
//         x
//     }};
// }

// const SSID: &str = env!("ESP_WIFI_SSID");
// const PASSWORD: &str = env!("ESP_WIFI_PASSWORD");

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    // generator version: 0.3.1

    rtt_target::rtt_init_defmt!();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let p = esp_hal::init(config);

    esp_alloc::heap_allocator!(size: 72 * 1024);

    let timer0 = SystemTimer::new(p.SYSTIMER);
    esp_hal_embassy::init(timer0.alarm0);

    info!("Embassy initialized!");

    // Turn on Test LED
    let eled = Output::new(p.GPIO5, Level::High, OutputConfig::default());
    // spawner
    //     .spawn(blinker(eled, Duration::from_millis(6000)))
    //     .ok();

    // Setup onBoard LED
    let rmt = Rmt::new(p.RMT, Rate::from_mhz(80)).unwrap();
    let rmt_buffer = smartLedBuffer!(1);
    let mut led = SmartLedsAdapter::new(rmt.channel0, p.GPIO48, rmt_buffer);

    // Make LED Blue
    led.write([RGB8::new(10, 10, 10)]).ok();

    info!("Setting up Screen");

    // Setup the Screen
    let dma_channel = p.DMA_CH0;
    let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) = dma_buffers!(32000);
    let dma_rx_buf = DmaRxBuf::new(rx_descriptors, rx_buffer).unwrap();
    let dma_tx_buf = DmaTxBuf::new(tx_descriptors, tx_buffer).unwrap();

    let spi = Spi::new(
        p.SPI2,
        Config::default()
            .with_frequency(Rate::from_khz(100))
            .with_mode(Mode::_0),
    )
    .unwrap()
    .with_sck(p.GPIO36)
    .with_mosi(p.GPIO35)
    .with_cs(p.GPIO39)
    .with_dma(dma_channel)
    .with_buffers(dma_rx_buf, dma_tx_buf)
    .into_async();

    let dc = Output::new(p.GPIO38, Level::High, OutputConfig::default());
    let rst = Output::new(p.GPIO37, Level::High, OutputConfig::default());
    let busy = Input::new(p.GPIO40, InputConfig::default());
    let mut display = EPD7in3f::new(spi, dc, rst, busy);

    info!("Init Screen");

    let _ = display.init().await;
    let _ = display.clear(draw::Color::Black).await;
    info!("Screen cleared");

    let _ = display.sleep().await;

    //
    // Setup Wifi
    //
    info!("Starting loop");

    loop {
        // let client_state = TcpClientState::<1, 1024, 1024>::new();
        // let tcp_client = TcpClient::new(stack, &client_state);
        // let dns_client = DnsSocket::new(stack);

        // let mut http_client = HttpClient::new(&tcp_client, &dns_client);

        // info!("sending requests");
        // let url = "http://192.168.68.75:8080/E-Paper_code/pic/output.epd";
        // if let Err(e) = draw::display_epd_streaming(&mut display, &mut http_client, url).await {
        //     warn!("Failed to display EPD: {:?}", e);
        // }

        // let mut request = match http_client.request(Method::GET, &url).await {
        //     Ok(req) => req,
        //     Err(e) => {
        //         error!("Failed to make HTTP request: {:?}", e);
        //         return; // handle the error
        //     }
        // };

        // let response = match request.send(&mut rx_buffer).await {
        //     Ok(resp) => resp,
        //     Err(_e) => {
        //         error!("Failed to send HTTP request");
        //         return; // handle the error;
        //     }
        // };

        // info!("Response body: {:?}", &response.content_length);
        // Timer::after(Duration::from_secs(5)).await;

        // display.init().await;

        // let res = response.body().read_to_end().await.unwrap();
        // // Display the EPD format image
        // if let Err(e) = display.display_epd(res).await {
        //     error!("Failed to display EPD: {:?}", e);
        // } else {
        //     info!("Display updated successfully");
        // }

        // Put the display to sleep when done
        Timer::after(Duration::from_secs(60)).await;
        info!("Writing Red!");
        led.write([RGB8::new(50, 0, 0)]).ok();
        let _ = display.init().await;
        let _ = display.clear(draw::Color::Red).await;
        info!("Done Red!");
        let _ = display.sleep().await;
        info!("Sleeping!");

        led.write([RGB8::new(10, 0, 0)]).ok();

        Timer::after(Duration::from_secs(60)).await;
        info!("Writing Green!");
        led.write([RGB8::new(0, 50, 0)]).ok();
        let _ = display.init().await;
        let _ = display.clear(draw::Color::Green).await;
        let _ = display.sleep().await;
        led.write([RGB8::new(0, 10, 0)]).ok();

        Timer::after(Duration::from_secs(60)).await;
        info!("Writing Blue!");
        led.write([RGB8::new(0, 0, 50)]).ok();
        let _ = display.init().await;
        let _ = display.clear(draw::Color::Blue).await;
        let _ = display.sleep().await;
        led.write([RGB8::new(0, 0, 10)]).ok();

        Timer::after(Duration::from_secs(60)).await;
        info!("Writing Yellow!");
        led.write([RGB8::new(0, 50, 50)]).ok();
        let _ = display.init().await;
        let _ = display.clear(draw::Color::Yellow).await;
        let _ = display.sleep().await;
        led.write([RGB8::new(0, 10, 10)]).ok();

        Timer::after(Duration::from_secs(60)).await;
        info!("Writing Orange!");
        led.write([RGB8::new(50, 0, 50)]).ok();
        let _ = display.init().await;
        let _ = display.clear(draw::Color::Orange).await;
        let _ = display.sleep().await;
        led.write([RGB8::new(10, 0, 10)]).ok();

        Timer::after(Duration::from_secs(60)).await;
        info!("Writing White!");
        led.write([RGB8::new(50, 50, 50)]).ok();
        let _ = display.init().await;
        let _ = display.clear(draw::Color::White).await;
        let _ = display.sleep().await;
        led.write([RGB8::new(10, 10, 10)]).ok();
    }
}

#[embassy_executor::task]
async fn blinker(mut led: Output<'static>, interval: Duration) {
    loop {
        // info!("Hello high!");

        led.set_high();
        Timer::after(interval).await;
        led.set_low();
        Timer::after(interval).await;
    }
}
