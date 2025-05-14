use defmt::{info, println, Format};
use embassy_net::dns::DnsSocket;
use embassy_net::tcp::client::TcpClient;
use embassy_time::{Duration, Timer};
use esp_hal::gpio::{Input, Output};
use esp_hal::spi::master::SpiDmaBus;
use esp_hal::Async;

use reqwless::client::HttpClient;
use reqwless::request::Method;
use reqwless::request::RequestBuilder;

// Display resolution
const EPD_WIDTH: u32 = 800;
const EPD_HEIGHT: u32 = 480;
pub(crate) const DISPLAY_BUFFER_SIZE: usize = (EPD_WIDTH * EPD_HEIGHT / 2) as usize;

const EPD_HEADER_SIZE: usize = 13;
const CHUNK_SIZE: usize = 32768;

// Static buffers to move data off the stack
static mut RX_BUFFER: [u8; 32768] = [0; 32768]; // 8KB for receiving HTTP data
static mut DISPLAY_BUFFER: [u8; EPD_WIDTH as usize * EPD_HEIGHT as usize / 2] =
    [0; EPD_WIDTH as usize * EPD_HEIGHT as usize / 2];
static mut RANGE_BUFFER: [u8; 32] = [0; 32]; // For range header string

#[derive(Debug, Format)]
pub enum Error {
    InvalidMagic,
    InvalidVersion,
    InvalidDimensions,
    BufferTooSmall,
    InvalidHeader,
    UnsupportedBitDepth,
    InvalidFileSize,
    HttpError,
    BmpParsing,
    WriteError,
    SpiError(esp_hal::spi::Error),
}

impl From<esp_hal::spi::Error> for Error {
    fn from(e: esp_hal::spi::Error) -> Self {
        Error::SpiError(e)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Color {
    Black = 0x000000,
    White = 0xffffff,
    Green = 0x00ff00,
    Blue = 0xff0000,
    Red = 0x0000ff,
    Yellow = 0x00ffff,
    Orange = 0x0080ff,
}

impl Color {
    pub fn to_byte(self) -> u8 {
        match self {
            Color::Black => 0x0,
            Color::White => 0x1,
            Color::Green => 0x2,
            Color::Blue => 0x3,
            Color::Red => 0x4,
            Color::Yellow => 0x5,
            Color::Orange => 0x6,
        }
    }
}

/// Converts RGB values to the display's color index
#[inline]
fn rgb_to_color_index(r: u8, g: u8, b: u8) -> u8 {
    match (r, g, b) {
        (0, 0, 0) => 0x0,       // Black
        (255, 255, 255) => 0x1, // White
        (0, 255, 0) => 0x2,     // Green
        (0, 0, 255) => 0x3,     // Blue
        (255, 0, 0) => 0x4,     // Red
        (255, 255, 0) => 0x5,   // Yellow
        (255, 128, 0) => 0x6,   // Orange
        _ => 0x1,               // Default to white for any other color
    }
}

pub struct EPD7in3f<'d> {
    spi: SpiDmaBus<'d, Async>,
    // cs: Output<'d>,
    dc: Output<'d>,
    rst: Output<'d>,
    busy: Input<'d>,
    width: u32,
    height: u32,
}

impl<'d> EPD7in3f<'d> {
    pub fn new(
        spi: SpiDmaBus<'d, Async>,
        // cs: Output<'d>,
        dc: Output<'d>,
        rst: Output<'d>,
        busy: Input<'d>,
    ) -> Self {
        Self {
            spi,
            // cs,
            dc,
            rst,
            busy,
            width: EPD_WIDTH,
            height: EPD_HEIGHT,
        }
    }

    // Hardware reset
    pub async fn reset(&mut self) {
        self.rst.set_high();
        Timer::after(Duration::from_millis(20)).await;
        self.rst.set_low();
        info!("Sent Wake command!");
        Timer::after(Duration::from_millis(10)).await;
        self.rst.set_high();
        Timer::after(Duration::from_millis(20)).await;
    }

    async fn send_command(&mut self, command: u8) -> Result<(), Error> {
        self.dc.set_low();
        // self.cs.set_low();
        self.spi.write_async(&[command]).await?;
        // self.cs.set_high();
        Ok(())
    }

    async fn send_data(&mut self, data: u8) -> Result<(), Error> {
        self.dc.set_high();
        // self.cs.set_low();
        self.spi.write_async(&[data]).await?;
        // self.cs.set_high();
        Ok(())
    }

    async fn send_data_slice(&mut self, data: &[u8]) -> Result<(), Error> {
        info!(
            "sending {} {} {} {} {} {}",
            data[0], data[1], data[2], data[3], data[4], data[5]
        );

        self.dc.set_high();
        // self.cs.set_low();
        self.spi.write_async(data).await?;
        // self.cs.set_high();
        Ok(())
    }

    async fn read_busy_h(&mut self) {
        while self.busy.is_low() {
            Timer::after(Duration::from_millis(5)).await;
        }
    }

    async fn turn_on_display(&mut self) -> Result<(), Error> {
        info!("Refreshing Screen!");

        self.send_command(0x04).await?; // POWER_ON
                                        // info!("Powering Screen...");
        self.read_busy_h().await;

        self.send_command(0x12).await?; // DISPLAY_REFRESH
        self.send_data(0x00).await?;
        // info!("Doing Refresh...");
        self.read_busy_h().await;

        self.send_command(0x02).await?; // POWER_OFF
        self.send_data(0x00).await?;
        self.read_busy_h().await;
        info!("Screen Refresh complete!");

        Ok(())
    }

    pub async fn init(&mut self) -> Result<(), Error> {
        info!("Display init...");

        self.reset().await;
        self.read_busy_h().await;
        Timer::after(Duration::from_millis(30)).await;

        // Initialize display registers
        self.send_command(0xAA).await?; // CMDH
        self.send_data(0x49).await?;
        self.send_data(0x55).await?;
        self.send_data(0x20).await?;
        self.send_data(0x08).await?;
        self.send_data(0x09).await?;
        self.send_data(0x18).await?;

        // Continue with all initialization commands from original driver
        self.send_command(0x01).await?;
        self.send_data(0x3F).await?;
        self.send_data(0x00).await?;
        self.send_data(0x32).await?;
        self.send_data(0x2A).await?;
        self.send_data(0x0E).await?;
        self.send_data(0x2A).await?;

        // ... (remaining initialization commands)
        self.send_command(0x00).await?;
        self.send_data(0x5F).await?;
        self.send_data(0x69).await?;

        self.send_command(0x03).await?;
        self.send_data(0x00).await?;
        self.send_data(0x54).await?;
        self.send_data(0x00).await?;
        self.send_data(0x44).await?;

        self.send_command(0x05).await?;
        self.send_data(0x40).await?;
        self.send_data(0x1F).await?;
        self.send_data(0x1F).await?;
        self.send_data(0x2C).await?;

        self.send_command(0x06).await?;
        self.send_data(0x6F).await?;
        self.send_data(0x1F).await?;
        self.send_data(0x1F).await?;
        self.send_data(0x22).await?;

        self.send_command(0x08).await?;
        self.send_data(0x6F).await?;
        self.send_data(0x1F).await?;
        self.send_data(0x1F).await?;
        self.send_data(0x22).await?;

        self.send_command(0x13).await?; // IPC
        self.send_data(0x00).await?;
        self.send_data(0x04).await?;

        self.send_command(0x30).await?;
        self.send_data(0x3C).await?;

        self.send_command(0x41).await?; //     # TSE
        self.send_data(0x00).await?;

        self.send_command(0x50).await?;
        self.send_data(0x3F).await?;

        self.send_command(0x60).await?;
        self.send_data(0x02).await?;
        self.send_data(0x00).await?;

        self.send_command(0x61).await?;
        self.send_data(0x03).await?;
        self.send_data(0x20).await?;
        self.send_data(0x01).await?;
        self.send_data(0xE0).await?;

        self.send_command(0x82).await?;
        self.send_data(0x1E).await?;

        self.send_command(0x84).await?;
        self.send_data(0x00).await?;

        self.send_command(0x86).await?; // AGID
        self.send_data(0x00).await?;

        self.send_command(0xE3).await?;
        self.send_data(0x2F).await?;

        self.send_command(0xE0).await?; //   # CCSET
        self.send_data(0x00).await?;

        self.send_command(0xE6).await?; //  # TSSET
        self.send_data(0x00).await?;
        info!("Init Complete.");

        Ok(())
    }

    pub async fn display(&mut self, buffer: &[u8]) -> Result<(), Error> {
        println!("Got: {} Want: {}", buffer.len(), DISPLAY_BUFFER_SIZE);
        if buffer.len() < DISPLAY_BUFFER_SIZE {
            return Err(Error::BufferTooSmall);
        }
        self.send_command(0x10).await?;
        self.send_data_slice(buffer).await?;
        self.turn_on_display().await?;
        Ok(())
    }

    // pub fn display_bmp(
    //     &mut self,
    //     bmp_data: &[u8],
    //     display_buffer: &mut [u8],
    // ) -> Result<(), BmpError> {
    //     // Check buffer size
    //     if display_buffer.len() < DISPLAY_BUFFER_SIZE {
    //         return Err(BmpError::BufferTooSmall);
    //     }

    //     // Parse BMP data
    //     let bmp_raw: RawBmp<'_> = RawBmp::from_slice(bmp_data).map_err(|_| BmpError::BmpParsing)?;
    //     let bmp: Bmp<'_, Rgb888> = Bmp::from_slice(bmp_data).map_err(|_| BmpError::BmpParsing)?;

    //     // Validate dimensions
    //     let header = bmp_raw.header();
    //     if header.image_size.width != EPD_WIDTH as u32
    //         || header.image_size.height != EPD_HEIGHT as u32
    //     {
    //         return Err(BmpError::InvalidDimensions);
    //     }

    //     // Process image data
    //     let width = header.image_size.width as u32;
    //     let height = header.image_size.height as u32;

    //     for y in 0..height {
    //         for x in (0..width).step_by(2) {
    //             // Get first pixel
    //             let pixel1 = bmp
    //                 .pixel(Point::new(x.try_into().unwrap(), y.try_into().unwrap()))
    //                 .ok_or(BmpError::BmpParsing)?;
    //             let color1 = rgb_to_color_index(pixel1.r(), pixel1.g(), pixel1.b());

    //             // Get second pixel (or use white if at the edge)
    //             let color2 = if x + 1 < width {
    //                 let pixel2 = bmp
    //                     .pixel(Point::new(
    //                         (x + 1).try_into().unwrap(),
    //                         y.try_into().unwrap(),
    //                     ))
    //                     .ok_or(BmpError::BmpParsing)?;
    //                 rgb_to_color_index(pixel2.r(), pixel2.g(), pixel2.b())
    //             } else {
    //                 1 // White padding for odd width
    //             };

    //             // Pack two 4-bit colors into one byte
    //             let display_byte = (color1 << 4) | color2;

    //             // Calculate position in display buffer
    //             let buffer_idx = (y * width + x) as usize / 2;
    //             display_buffer[buffer_idx] = display_byte;
    //         }
    //     }

    //     Ok(())
    // }

    /// Reads our custom EPD format and displays it
    pub async fn display_epd(&mut self, data: &[u8]) -> Result<(), Error> {
        // Check minimum size for header (magic + version + dimensions)
        if data.len() < 13 {
            return Err(Error::BufferTooSmall);
        }

        // Check magic number "EPD7"
        if &data[0..4] != b"EPD7" {
            return Err(Error::InvalidMagic);
        }

        // Check version
        if data[4] != 1 {
            return Err(Error::InvalidVersion);
        }

        // Read dimensions
        let width = u32::from_le_bytes(data[5..9].try_into().unwrap());
        let height = u32::from_le_bytes(data[9..13].try_into().unwrap());

        // Verify dimensions
        if width != EPD_WIDTH || height != EPD_HEIGHT {
            return Err(Error::InvalidDimensions);
        }

        // The rest of the data is already in the correct format for our display
        // as we packed it that way in the converter
        let display_data = &data[13..];

        // Calculate expected data size (width * height / 2 because we pack 2 pixels per byte)
        let expected_size = ((800 * 480) / 2) as usize;
        if display_data.len() < expected_size {
            println!("{} >> {}", expected_size, display_data.len());
            return Err(Error::BufferTooSmall);
        }

        // Send the data to display
        self.display(display_data).await;

        Ok(())
    }

    pub async fn clear(&mut self, color: Color) -> Result<(), Error> {
        // Fill buffer with color
        let color_byte = (color.to_byte() << 4) | color.to_byte();
        info!("Sending Image Data");

        self.send_command(0x10).await?;
        // Send the same color byte for the entire display
        // We send width * height / 2 bytes because each byte contains 2 pixels
        for _ in 0..(EPD_WIDTH * EPD_HEIGHT / 2) {
            self.send_data(color_byte).await?;
        }
        info!("Sending Done!");

        self.turn_on_display().await?;

        Ok(())
    }

    pub async fn sleep(&mut self) -> Result<(), Error> {
        self.send_command(0x07).await?; // DEEP_SLEEP
        self.send_data(0xA5).await?;
        Timer::after(Duration::from_millis(2000)).await;
        Ok(())
    }
}

pub async fn display_epd_streaming(
    display: &mut EPD7in3f<'_>,
    http_client: &mut HttpClient<'_, TcpClient<'_, 1, 1024, 1024>, DnsSocket<'_>>,
    url: &str,
) -> Result<(), Error> {
    // SAFETY: We know we have exclusive access to these static buffers
    let rx_buffer = unsafe { &mut RX_BUFFER };
    // let display_buffer = unsafe { &mut DISPLAY_BUFFER };
    let range_buffer = unsafe { &mut RANGE_BUFFER };

    // First get the header
    {
        let range_str = "bytes=0-12";
        let range_tuple = ("Range", range_str);
        let headers = [range_tuple];
        let mut request = http_client
            .request(Method::GET, url)
            .await
            .map_err(|_| Error::HttpError)?;
        let mut request = request.headers(&headers);
        let response = request
            .send(rx_buffer)
            .await
            .map_err(|_| Error::HttpError)?;

        let header = response.body().body_buf;

        // Verify header
        if header.len() < 13 {
            return Err(Error::BufferTooSmall);
        }
        if &header[0..4] != b"EPD7" {
            return Err(Error::InvalidMagic);
        }
        info!("image valid!");

        if header[4] != 1 {
            return Err(Error::InvalidVersion);
        }
    }

    display.send_command(0x10).await?; // Start data transmission

    let mut bytes_read = 0;
    // const CHUNK_SIZE: usize = 8192;
    const CHUNK_SIZE: usize = 32768;
    const TOTAL_SIZE: usize = EPD_WIDTH as usize * EPD_HEIGHT as usize / 2;

    // Then in your main function:
    while bytes_read < TOTAL_SIZE {
        // Clear range buffer
        range_buffer.fill(0);

        // Create range string manually
        let chunk_start = 13 + bytes_read; // 13 is header size
        let chunk_end = chunk_start + CHUNK_SIZE.min(TOTAL_SIZE - bytes_read) - 1;

        // Write "bytes="
        range_buffer[0..6].copy_from_slice(b"bytes=");
        let mut pos = 6;

        // Write start number
        pos += write_number(range_buffer, chunk_start, pos);

        // Write "-"
        range_buffer[pos] = b'-';
        pos += 1;

        // Write end number
        pos += write_number(range_buffer, chunk_end, pos);

        let range_str =
            core::str::from_utf8(&range_buffer[..pos]).map_err(|_| Error::WriteError)?;

        let chunk_data = {
            let range_tuple = ("Range", range_str);
            let headers = [range_tuple];

            let mut request = http_client
                .request(Method::GET, url)
                .await
                .map_err(|_| Error::HttpError)?;
            let mut request = request.headers(&headers);
            let response = request
                .send(rx_buffer)
                .await
                .map_err(|_| Error::HttpError)?;

            response.body().body_buf
        };

        display.send_data_slice(chunk_data).await?;
        bytes_read += chunk_data.len();
    }
    info!("display on");

    // Finish display update
    display.turn_on_display().await?;

    Ok(())
}

// Add this helper function
fn write_number(buffer: &mut [u8], mut num: usize, offset: usize) -> usize {
    let mut digits = [0u8; 20]; // Max length of usize
    let mut i = 0;

    if num == 0 {
        buffer[offset] = b'0';
        return 1;
    }

    while num > 0 {
        digits[i] = (num % 10) as u8 + b'0';
        num /= 10;
        i += 1;
    }

    for j in 0..i {
        buffer[offset + j] = digits[i - 1 - j];
    }
    i
}

// pub async fn display_epd_streaming(
//     display: &mut EPD7in3f<'_>,
//     http_client: &mut HttpClient<'_, TcpClient<'_, 1, 1024, 1024>, DnsSocket<'_>>,
//     rx_buffer: &mut [u8],
// ) -> Result<(), Error> {
//     // First, get just the header
//     let url = "http://192.168.68.75:8080/E-Paper_code/pic/output.epd";
//     let mut request: reqwless::client::HttpRequestHandle<
//         '_,
//         embassy_net::tcp::client::TcpConnection<'_, 1, 1024, 1024>,
//         (),
//     > = http_client
//         .request(Method::GET, url)
//         .await
//         .map_err(|_err| Error::BmpParsing)?;
//     request.headers(&[("Range", "bytes=0-12")]);

//     let response = request
//         .send(rx_buffer)
//         .await
//         .map_err(|_err| Error::BmpParsing)?;

//     let header = response.body().body_buf;

//     // Verify header
//     if header.len() < EPD_HEADER_SIZE {
//         return Err(Error::BufferTooSmall);
//     }
//     if &header[0..4] != b"EPD7" {
//         return Err(Error::InvalidMagic);
//     }
//     if header[4] != 1 {
//         return Err(Error::InvalidVersion);
//     }

//     let width = u32::from_le_bytes(header[5..9].try_into().unwrap());
//     let height = u32::from_le_bytes(header[9..13].try_into().unwrap());

//     if width != EPD_WIDTH || height != EPD_HEIGHT {
//         return Err(Error::InvalidDimensions);
//     }

//     // Initialize display for data
//     display.init().await?;
//     display.send_command(0x10).await?; // Start data transmission

//     // Now stream the image data in chunks
//     let total_data_size = (width * height / 2) as usize;
//     let mut bytes_read = 0;

//     // Pre-calculate all the range headers we might need
//     const MAX_CHUNKS: usize = 24; // For 192KB file with 8KB chunks
//     let mut range_headers: [heapless::String<32>; MAX_CHUNKS] =
//         [const { heapless::String::new() }; MAX_CHUNKS];

//     for i in 0..MAX_CHUNKS {
//         let chunk_start = EPD_HEADER_SIZE + (i * CHUNK_SIZE);
//         let chunk_end = chunk_start + CHUNK_SIZE - 1;
//         // Use heapless string formatting
//         if let Ok(()) = write!(range_headers[i], "bytes={}-{}", chunk_start, chunk_end) {
//             // Successfully created range header
//         } else {
//             return Err(Error::BufferTooSmall);
//         }
//     }

//     let mut chunk_index = 0;
//     while bytes_read < total_data_size {
//         if chunk_index >= MAX_CHUNKS {
//             return Err(Error::BufferTooSmall);
//         }

//         // Request the next chunk
//         let mut request = http_client
//             .request(Method::GET, url)
//             .await
//             .map_err(|_err| Error::BmpParsing)?;
//         request.headers(&[("Range", range_headers[chunk_index].as_str())]);

//         let response = request
//             .send(rx_buffer)
//             .await
//             .map_err(|_err| Error::BmpParsing)?;

//         let chunk_data = response.body().body_buf;

//         // Send this chunk to the display
//         display.send_data_slice(chunk_data).await;

//         bytes_read += chunk_data.len();
//         chunk_index += 1;
//     }

//     // Finish the display update
//     display.turn_on_display().await;

//     Ok(())
// }

// // Helper function to convert RGB image data to display format
// pub fn convert_to_display_buffer(rgb_data: &[u8]) -> Result<HVec<u8, DISPLAY_BUFFER_SIZE>, ()> {
//     let mut display_buffer = HVec::new();

//     for i in (0..rgb_data.len()).step_by(6) {
//         if i + 5 >= rgb_data.len() {
//             break;
//         }

//         // Convert first pixel RGB to color index
//         let color1 = match (rgb_data[i], rgb_data[i + 1], rgb_data[i + 2]) {
//             (0, 0, 0) => 0x0,       // Black
//             (255, 255, 255) => 0x1, // White
//             (0, 255, 0) => 0x2,     // Green
//             (0, 0, 255) => 0x3,     // Blue
//             (255, 0, 0) => 0x4,     // Red
//             (255, 255, 0) => 0x5,   // Yellow
//             (255, 128, 0) => 0x6,   // Orange
//             _ => 0x1,               // Default to white
//         };

//         // Convert second pixel RGB to color index
//         let color2 = match (rgb_data[i + 3], rgb_data[i + 4], rgb_data[i + 5]) {
//             (0, 0, 0) => 0x0,
//             (255, 255, 255) => 0x1,
//             (0, 255, 0) => 0x2,
//             (0, 0, 255) => 0x3,
//             (255, 0, 0) => 0x4,
//             (255, 255, 0) => 0x5,
//             (255, 128, 0) => 0x6,
//             _ => 0x1,
//         };

//         // Pack two 4-bit colors into one byte
//         display_buffer
//             .extend_from_slice(&[(color1 << 4) | color2])
//             .map_err(|_| ())?;
//     }

//     Ok(display_buffer)
// }
