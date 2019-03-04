//! TODO: heavily document the application here.
#![no_main]
#![no_std]

use rtfm::{app, Instant};
use s32k144::Interrupt;
use s32k144evb::wdog;

pub mod adc;
pub mod can;
pub mod csec;
pub mod panic;
pub mod scg;
pub mod utils;

const PERIOD: u32 = 8_000;
const PLAINKEY: [u8; 16] = [
    0x2b, 0x7e, 0x15, 0x16, 0x28, 0xae, 0xd2, 0xa6, 0xab, 0xf7, 0x15, 0x88, 0x09, 0xcf, 0x4f, 0x3c,
];

#[app(device = s32k144)]
const APP: () = {
    // Resources
    static mut ADC: adc::ADC = ();
    static mut CSEC: csec::CSEc = ();
    static mut CAN: can::CAN = ();

    #[init(schedule = [poll_sensor])]
    fn init() -> init::LateResources {
        // Disable watchdog
        let wdog_settings = wdog::WatchdogSettings {
            enable: false,
            ..Default::default()
        };
        let _wdog = wdog::Watchdog::init(&device.WDOG, wdog_settings).unwrap();

        // Configure clocks as required for CAN-FD.
        scg::configure_spll_clock(&device.SCG);

        // Initialize ADC and CAN-FD
        let adc = adc::ADC::init(&device.PCC, device.ADC0);
        let can = can::CAN::init(
            &device.PCC,
            device.CAN0,
            &device.PORTE,
            &device.PORTD,
            &device.PTD,
        );

        // Load plainkey and initialize RNG
        let csec = csec::CSEc::init(device.FTFC, device.CSE_PRAM);
        csec.init_rng().unwrap();
        csec.load_plainkey(&PLAINKEY).unwrap();

        schedule
            .poll_sensor(Instant::now() + PERIOD.cycles())
            .unwrap();

        init::LateResources {
            ADC: adc,
            CAN: can,
            CSEC: csec,
        }
    }

    #[idle]
    fn idle() -> ! {
        // Sleep
        loop {
            rtfm::pend(Interrupt::DMA0);
        }
    }

    #[task(resources = [ADC, CAN, CSEC], schedule = [poll_sensor])]
    fn poll_sensor() {
        let adc = resources.ADC;
        let can = resources.CAN;
        let csec = resources.CSEC;

        let sensor_values: [u8; 2] = split_u16_to_byte_array(adc.read());

        // Randomize our initialization vector.
        let mut init_vec: [u8; 16] = [0; 16];
        csec.generate_rnd(&mut init_vec).unwrap();

        // Encrypt the sensor data.
        let mut payload: [u8; 16 + 2] = [0; 16 + 2];
        csec.encrypt_cbc(&sensor_values, &init_vec, &mut payload[16..])
            .unwrap();

        // Transmit the payload, with a prefixed initialization vector.
        payload[..16].clone_from_slice(&init_vec);
        can.transmit(&payload);

        schedule.poll_sensor(scheduled + PERIOD.cycles()).unwrap();
    }

    // Interrupt handlers used to dispatch software tasks
    extern "C" {
        fn DMA0();
    }
};

fn split_u16_to_byte_array(n: u16) -> [u8; 2] {
    let x: u8 = ((n >> 8) & 0xff) as u8;
    let y: u8 = (n & 0xff) as u8;
    [x, y]
}
