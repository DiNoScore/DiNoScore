use glib::Sender;
use std::{any::Any, error::Error};

use midi_event::*;
use portmidi as pm;

pub enum PageEvent {
	Next,
	Previous,
}

pub fn run(midi_tx: Sender<PageEvent>) -> Result<Box<dyn Any>, Box<dyn Error>> {
	/* PortMidi is awful. Not my fault. Please, someone make RtMidi bindings for Rust and save us! */
	std::thread::spawn(move || {
		let pm = pm::PortMidi::new().unwrap();
		let mut suitable_ports = pm
			.devices()
			.unwrap()
			.into_iter()
			.filter(|d| d.direction() == portmidi::Direction::Input)
			.filter(|d| !d.name().contains("Through"))
			.inspect(|d| println!("Listening for MIDI pedals on {:?}", d))
			.map(|d| pm.input_port(d, 12).unwrap())
			.collect::<Vec<_>>();
		if suitable_ports.is_empty() {
			println!("No midi ports found to listen on");
		}

		loop {
			std::thread::sleep(std::time::Duration::from_millis(75));
			for port in &mut suitable_ports {
				while let Ok(Some(event)) = port.read() {
					if let Some(event) = MidiEvent::parse(&[
						event.message.status,
						event.message.data1,
						event.message.data2,
					]) {
						dbg!(&event);
						match event.event {
							MidiEventType::Controller(67, 127) => {
								midi_tx.send(PageEvent::Next).unwrap();
							},
							MidiEventType::Controller(66, 127) => {
								midi_tx.send(PageEvent::Previous).unwrap();
							},
							_ => {},
						}
					}
				}
			}
		}
	});

	/* In other implementations, this can be used to keep an object alive for the time the program runs */
	Ok(Box::new(()))
}
