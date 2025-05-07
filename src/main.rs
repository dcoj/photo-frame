#![no_std]
#![no_main]
mod led;
use defmt::info;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::rmt::Rmt;
use esp_hal::time::Rate;
use esp_hal::timer::systimer::SystemTimer;
use esp_hal::timer::timg::TimerGroup;
use led::SmartLedsAdapter;
use panic_rtt_target as _;
use smart_leds::RGB8;
use smart_leds::{
    brightness, gamma,
    hsv::{hsv2rgb, Hsv},
    SmartLedsWrite,
};

extern crate alloc;

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

    let timer1 = TimerGroup::new(p.TIMG0);
    let _init = esp_wifi::init(timer1.timer0, esp_hal::rng::Rng::new(p.RNG), p.RADIO_CLK).unwrap();

    let led = Output::new(p.GPIO5, Level::High, OutputConfig::default());
    spawner
        .spawn(blinker(led, Duration::from_millis(6000)))
        .ok();

    // Assuming GPIO18 is your data pin
    // Configure RMT peripheral globally
    let rmt = Rmt::new(p.RMT, Rate::from_mhz(80)).unwrap();

    let rmt_buffer = smartLedBuffer!(1);
    let mut led = SmartLedsAdapter::new(rmt.channel0, p.GPIO48, rmt_buffer);

    // Simple color setting
    led.write([RGB8::new(0, 0, 16)]).ok();
}

#[embassy_executor::task]
async fn blinker(mut led: Output<'static>, interval: Duration) {
    loop {
        info!("Hello high!");

        led.set_high();
        Timer::after(interval).await;
        led.set_low();
        Timer::after(interval).await;
    }
}
