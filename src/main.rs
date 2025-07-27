#![no_std]
#![no_main]
#![allow(
    clippy::manual_div_ceil,
    reason = "Allowed as waiting on https://github.com/rust-lang/rust-clippy/pull/14666"
)]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![feature(allocator_api)]

use defmt::info;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};

extern crate alloc;
use panic_rtt_target as _;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
// esp_bootloader_esp_idf::esp_app_desc!();

mod draw;
mod led;
mod wifi;
use alloc::vec::Vec;

use defmt::{error, println, warn};
use embassy_net::{
    dns::DnsSocket,
    tcp::client::{TcpClient, TcpClientState},
    StackResources,
};
use esp_hal::{
    clock::CpuClock,
    dma::{DmaRxBuf, DmaTxBuf},
    dma_buffers,
    gpio::{Input, InputConfig, Level, Output, OutputConfig},
    psram::PsramConfig,
    rmt::Rmt,
    spi::{
        master::{Config, Spi},
        Mode,
    },
    time::Rate,
    timer::{systimer::SystemTimer, timg::TimerGroup},
};

use draw::EPD7in3f;
use esp_wifi::{config::PowerSaveMode, init, EspWifiController};
use led::SmartLedsAdapter;
use reqwless::{client::HttpClient, request::Method};
use smart_leds::{SmartLedsWrite, RGB8};
use wifi::{connection, net_task};

macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    rtt_target::rtt_init_defmt!();
    info!("Embassy Hello!");

    static PSRAM_ALLOCATOR: esp_alloc::EspHeap = esp_alloc::EspHeap::empty();
    let psram_config = PsramConfig::default();

    let config = esp_hal::Config::default()
        .with_cpu_clock(CpuClock::max())
        .with_psram(psram_config);

    let p = esp_hal::init(config);
    esp_alloc::heap_allocator!(size: 100 * 1024);

    let (start, size) = esp_hal::psram::psram_raw_parts(&p.PSRAM);
    unsafe {
        PSRAM_ALLOCATOR.add_region(esp_alloc::HeapRegion::new(
            start,
            size,
            esp_alloc::MemoryCapability::External.into(),
        ));
    }

    info!("Starting Wifi");
    let timg0 = TimerGroup::new(p.TIMG0);
    let mut rng = esp_hal::rng::Rng::new(p.RNG);

    let esp_wifi_ctrl = &*mk_static!(
        EspWifiController<'static>,
        init(timg0.timer0, rng, p.RADIO_CLK).unwrap()
    );

    let (mut wifi_controller, interfaces) =
        esp_wifi::wifi::new(esp_wifi_ctrl, p.WIFI).expect("Failed to initialize WIFI controller");
    let wifi_interface = interfaces.sta;
    wifi_controller
        .set_mode(esp_wifi::wifi::WifiMode::Sta)
        .expect("Failed to set wifi mode");
    wifi_controller
        .set_power_saving(PowerSaveMode::Minimum)
        .expect("Failed to set power mode");

    let systimer = SystemTimer::new(p.SYSTIMER);
    esp_hal_embassy::init(systimer.alarm0);

    info!("Embassy initialized!");

    // Turn on a Test LED
    // let eled = Output::new(p.GPIO5, Level::High, OutputConfig::default());
    // spawner
    //     .spawn(blinker(eled, Duration::from_millis(6000)))
    //     .ok();

    // Setup onBoard LED
    let rmt = Rmt::new(p.RMT, Rate::from_mhz(80)).unwrap();
    let rmt_buffer = smartLedBuffer!(1);
    let mut led = SmartLedsAdapter::new(rmt.channel0, p.GPIO48, rmt_buffer);

    // Make LED Blue
    led.write([RGB8::new(0, 0, 0)]).ok();

    let stats: esp_alloc::HeapStats = esp_alloc::HEAP.stats();
    // HeapStats implements the Display and defmt::Format traits, so you can pretty-print the heap stats.
    println!("{}", stats);

    info!("Setting up Screen");

    // Setup the Screen
    let dma_channel = p.DMA_CH0;
    let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) = dma_buffers!(32000);
    let dma_rx_buf = DmaRxBuf::new(rx_descriptors, rx_buffer).unwrap();
    let dma_tx_buf = DmaTxBuf::new(tx_descriptors, tx_buffer).unwrap();

    let spi = Spi::new(p.SPI2, Config::default().with_mode(Mode::_0))
        .unwrap()
        .with_sck(p.GPIO12)
        .with_mosi(p.GPIO11)
        .with_cs(p.GPIO10)
        .with_dma(dma_channel)
        .with_buffers(dma_rx_buf, dma_tx_buf)
        .into_async();

    let dc = Output::new(p.GPIO14, Level::High, OutputConfig::default());
    let rst = Output::new(p.GPIO13, Level::High, OutputConfig::default());
    let busy = Input::new(p.GPIO9, InputConfig::default());
    let mut display = EPD7in3f::new(spi, dc, rst, busy);

    //
    // Setup Wifi
    //

    let config = embassy_net::Config::dhcpv4(Default::default());
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;

    // Init network stack
    let (stack, runner) = embassy_net::new(
        wifi_interface,
        config,
        mk_static!(StackResources<3>, StackResources::<3>::new()),
        seed,
    );

    spawner.spawn(connection(wifi_controller)).ok();
    spawner.spawn(net_task(runner)).ok();

    info!("Waiting to start WiFi...");

    loop {
        if stack.is_link_up() {
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }

    info!("Waiting to get IP address...");
    loop {
        if let Some(config) = stack.config_v4() {
            info!("Got IP: {}", config.address);
            break;
        }
        Timer::after(Duration::from_millis(500)).await;
    }

    let stats: esp_alloc::HeapStats = esp_alloc::HEAP.stats();
    println!("{}", stats);

    loop {
        let client_state = TcpClientState::<1, 1024, 1024>::new();
        let tcp_client = TcpClient::new(stack, &client_state);
        let dns_client = DnsSocket::new(stack);

        let mut http_client = HttpClient::new(&tcp_client, &dns_client);

        info!("sentting up requests");
        let url = "http://192.168.68.66:3005/recent";
        // if let Err(e) = draw::display_epd_streaming(&mut display, &mut http_client, url).await {
        //     warn!("Failed to display EPD: {:?}", e);
        // }

        let mut request = http_client.request(Method::GET, url).await.unwrap();

        let stats: esp_alloc::HeapStats = esp_alloc::HEAP.stats();
        // HeapStats implements the Display and defmt::Format traits, so you can pretty-print the heap stats.
        println!("{}", stats);
        let stats: esp_alloc::HeapStats = PSRAM_ALLOCATOR.stats();
        // HeapStats implements the Display and defmt::Format traits, so you can pretty-print the heap stats.
        println!("PSRAM: {}", stats);

        info!("init vector");
        Timer::after(Duration::from_secs(2)).await;

        let mut vec = Vec::with_capacity_in(200000, &PSRAM_ALLOCATOR);
        // It seems to be more reliable to pre-fill the external memory
        vec.resize(vec.capacity(), 0_u8);

        info!("done vector");
        Timer::after(Duration::from_secs(2)).await;

        let stats: esp_alloc::HeapStats = esp_alloc::HEAP.stats();
        // HeapStats implements the Display and defmt::Format traits, so you can pretty-print the heap stats.
        println!("{}", stats);
        let stats: esp_alloc::HeapStats = PSRAM_ALLOCATOR.stats();
        // HeapStats implements the Display and defmt::Format traits, so you can pretty-print the heap stats.
        println!("PSRAM: {}", stats);

        info!("send request");
        Timer::after(Duration::from_secs(1)).await;

        let response = match request.send(&mut vec).await {
            Ok(file) => file,
            Err(e) => {
                info!("Failed to make request");
                info!("Err: {}", e);
                return;
            }
        };
        info!("sent request");

        let body = response.body().read_to_end().await.unwrap();
        println!("Got body: {}", body.len());

        let stats: esp_alloc::HeapStats = esp_alloc::HEAP.stats();
        // HeapStats implements the Display and defmt::Format traits, so you can pretty-print the heap stats.
        println!("{}", stats);

        Timer::after(Duration::from_secs(10)).await;
        led.write([RGB8::new(0, 0, 10)]).ok();
        let _ = display.init().await;

        // // Display the EPD format image
        // println!("display sen: {=[u8]:x}", body[0..10]);

        // for a in 0..body.len() / 1024 {
        //     println!("{=[u8]:x}", body[(a + 1) * 1024..((a + 1) * 1024) + 1024]);
        // }

        if let Err(e) = display.display_epd(body).await {
            error!("Failed to display EPD: {:?}", e);
        } else {
            info!("Display updated successfully");
        }
        info!("Now Sleeping!");
        let _ = display.sleep().await;
        led.write([RGB8::new(0, 0, 0)]).ok();

        // Put the display to sleep for 1hr when done
        Timer::after(Duration::from_secs(60 * 60)).await;

        // info!("Writing Red!");
        // led.write([RGB8::new(50, 0, 0)]).ok();
        // let _ = display.init().await;
        // let _ = display.clear(draw::Color::Red).await;
        // info!("Done Red!");
        // let _ = display.sleep().await;
        // info!("Sleeping!");

        // led.write([RGB8::new(10, 0, 0)]).ok();

        // Timer::after(Duration::from_secs(60)).await;
        // info!("Writing Green!");
        // led.write([RGB8::new(0, 50, 0)]).ok();
        // let _ = display.init().await;
        // let _ = display.clear(draw::Color::Green).await;
        // let _ = display.sleep().await;
        // led.write([RGB8::new(0, 10, 0)]).ok();

        // Timer::after(Duration::from_secs(60)).await;
        // info!("Writing Blue!");
        // led.write([RGB8::new(0, 0, 50)]).ok();
        // let _ = display.init().await;
        // let _ = display.clear(draw::Color::Blue).await;
        // let _ = display.sleep().await;
        // led.write([RGB8::new(0, 0, 10)]).ok();

        // Timer::after(Duration::from_secs(60)).await;
        // info!("Writing Black!");
        // led.write([RGB8::new(10, 10, 10)]).ok();
        // let _ = display.init().await;
        // let _ = display.clear(draw::Color::Black).await;
        // let _ = display.sleep().await;
        // led.write([RGB8::new(5, 5, 5)]).ok();

        // Timer::after(Duration::from_secs(60)).await;
        // info!("Writing Yellow!");
        // led.write([RGB8::new(0, 50, 50)]).ok();
        // let _ = display.init().await;
        // let _ = display.clear(draw::Color::Yellow).await;
        // let _ = display.sleep().await;
        // led.write([RGB8::new(0, 10, 10)]).ok();

        // Timer::after(Duration::from_secs(60)).await;
        // info!("Writing Orange!");
        // led.write([RGB8::new(50, 0, 50)]).ok();
        // let _ = display.init().await;
        // let _ = display.clear(draw::Color::Orange).await;
        // let _ = display.sleep().await;
        // led.write([RGB8::new(10, 0, 10)]).ok();

        // Timer::after(Duration::from_secs(60)).await;
        // info!("Writing White!");
        // led.write([RGB8::new(50, 50, 50)]).ok();
        // let _ = display.init().await;
        // let _ = display.clear(draw::Color::White).await;
        // let _ = display.sleep().await;
        // led.write([RGB8::new(10, 10, 10)]).ok();
    }
}

#[embassy_executor::task]
async fn blinker(mut led: Output<'static>, interval: Duration) {
    loop {
        warn!("Hello high!");

        led.set_high();
        Timer::after(interval).await;
        led.set_low();
        Timer::after(interval).await;
    }
}
