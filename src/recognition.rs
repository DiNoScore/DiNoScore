use itertools::Itertools;
use super::*;

const PB_PATH: &str = "./res/2019-05-16_faster-rcnn-inception-resnet-v2.pb";

static DETECTION_GRAPH: once_cell::sync::Lazy<tensorflow::Graph> =
	once_cell::sync::Lazy::new(|| {
		use std::io::Read;
		use tensorflow as tf;

		let mut detection_graph = tf::Graph::new();
		let mut proto = Vec::new();
		std::fs::File::open(PB_PATH)
			.unwrap()
			.read_to_end(&mut proto)
			.unwrap();
		detection_graph
			.import_graph_def(&proto, &tf::ImportGraphDefOptions::new())
			.unwrap();
		detection_graph
	});

#[derive(Debug, Clone)]
pub struct RelativeStaff {
	pub left: f64,
	pub top: f64,
	pub right: f64,
	pub bottom: f64,
}

impl RelativeStaff {
	pub fn into_staff(self, page: collection::PageIndex, width: f64, height: f64) -> collection::Staff {
		collection::Staff {
			page,
			start: (self.left * width, self.top * height),
			end: (self.right * width, self.bottom * height),
		}
	}
}

pub fn recognize_staves(image: &gdk_pixbuf::Pixbuf) -> Vec<RelativeStaff> {
	assert!(!image.get_has_alpha());
	assert!(image.get_n_channels() == 3);
	assert!(image.get_colorspace() == gdk_pixbuf::Colorspace::Rgb);

	let image_bytes = image.read_pixel_bytes().unwrap();
	let image_height = image.get_height();
	let image_width = image.get_width();

	let (num_detections, detection_boxes, detection_scores) = {
		use tensorflow as tf;

		println!("A");
		let detection_graph = &DETECTION_GRAPH;

		let image_tensor =
			tf::Tensor::new(&[1, image_height as u64, image_width as u64, 3])
				.with_values(&image_bytes)
				.unwrap();
		println!("A2");

		let mut session = tf::Session::new(&tf::SessionOptions::new(), &detection_graph).unwrap();
		println!("A3");
		let mut session_args = tf::SessionRunArgs::new();
		println!("A4");
		session_args.add_feed::<u8>(
			&detection_graph
				.operation_by_name("image_tensor")
				.unwrap()
				.unwrap(),
			0,
			&image_tensor,
		);
		println!("B");

		let num_detections = session_args.request_fetch(
			&detection_graph
				.operation_by_name("num_detections")
				.unwrap()
				.unwrap(),
			0,
		);
		let detection_boxes = session_args.request_fetch(
			&detection_graph
				.operation_by_name("detection_boxes")
				.unwrap()
				.unwrap(),
			0,
		);
		let detection_scores = session_args.request_fetch(
			&detection_graph
				.operation_by_name("detection_scores")
				.unwrap()
				.unwrap(),
			0,
		);
		let detection_classes = session_args.request_fetch(
			&detection_graph
				.operation_by_name("detection_classes")
				.unwrap()
				.unwrap(),
			0,
		);

		println!("C");
		session.run(&mut session_args).unwrap();
		println!("D");

		/* We could probably extract better results by making more use of all that information */
		let num_detections = session_args.fetch::<f32>(num_detections).unwrap();
		let detection_boxes = session_args.fetch::<f32>(detection_boxes).unwrap();
		let detection_scores = session_args.fetch::<f32>(detection_scores).unwrap();
		let _detection_classes = session_args.fetch::<f32>(detection_classes).unwrap();

		println!("E");
		session.close().unwrap();
		println!("F");

		(num_detections, detection_boxes, detection_scores)
	};
	println!("Checkpoint");

	let mut bars = Vec::<RelativeStaff>::new();

	for i in 0..(num_detections[0] as usize) {
		if detection_scores[i] > 0.6 {
			let detected = &detection_boxes[i * 4..i * 4 + 4];
			let y1 = detected[0] * image.get_height() as f32;
			let x1 = detected[1] * image.get_width() as f32;
			let y2 = detected[2] * image.get_height() as f32;
			let x2 = detected[3] * image.get_width() as f32;

			bars.push(RelativeStaff {
				left: x1 as f64,
				top: y1 as f64,
				bottom: y2 as f64,
				right: x2 as f64,
			});
		}
	}

	let scale_x = 1.0 / image.get_width() as f64;
	let scale_y = 1.0 / image.get_height() as f64;

	for bar in &mut bars {
		bar.left *= scale_x;
		bar.top *= scale_y;
		bar.right *= scale_x;
		bar.bottom *= scale_y;
	}

	/* Group them by staff */
	let mut bars = bars.into_iter().enumerate().collect::<Vec<_>>();

	while { /* do */
		let mut changed = false;
		for i in 0..bars.len() {
			for j in 0..bars.len() {
				if i == j {
					continue;
				}
				// This is safe thanks to the index check above
				let bar1 = & unsafe { &*(&bars as *const Vec<(usize, RelativeStaff)>) }[i];
				let bar2 = &mut unsafe { &mut *(&mut bars as *mut Vec<(usize, RelativeStaff)>) }[j];
				let c1 = (bar1.1.top + bar1.1.bottom) / 2.0;
				let c2 = (bar2.1.top + bar2.1.bottom) / 2.0;
				if c1 > bar2.1.top && c1 < bar2.1.bottom
						&& c2 > bar1.1.top && c2 < bar1.1.bottom
						&& bar1.0 != bar2.0 {
					changed = true;
					bar2.0 = bar1.0;
				}
			}
		}
		/* while */
		changed
	} {};

	let staves = bars.into_iter().into_group_map();

	/* Merge them */
	use reduce::Reduce;
	let mut staves: Vec<RelativeStaff> = staves.into_iter().filter_map(
		|staves| {
			staves.1.into_iter().reduce(|a, b| RelativeStaff {
				left: a.left.min(b.left),
				right: a.right.max(b.right),
				top: a.top.min(b.top),
				bottom: a.bottom.max(b.bottom),
			})
		})
		.collect();
	staves.sort_by(|a, b| a.top.partial_cmp(&b.top).unwrap());

	/* Overlapping is bad */
	(0..staves.len()).collect::<Vec<_>>()
		.windows(2)
		.for_each(|idx| {
			macro_rules! staff_a (() => {staves[idx[0]]});
			macro_rules! staff_b (() => {staves[idx[1]]});

			if staff_a!().bottom > staff_b!().top
				&& /* 90% horizontal overlap */
				(f64::min(staff_a!().right, staff_b!().right) - f64::max(staff_a!().left, staff_b!().left)) / (f64::max(staff_a!().right, staff_b!().right) - f64::min(staff_a!().left, staff_b!().left)) > 0.9
			{
				let center = (staff_a!().bottom + staff_b!().top) / 2.0;
				staff_a!().bottom = center;
				staff_b!().top = center;
			}
		});

	/* Fixup fuckups */
	for staff in &mut staves {
		if staff.top > staff.bottom {
			std::mem::swap(&mut staff.top, &mut staff.bottom);
		}
		if staff.left > staff.right {
			std::mem::swap(&mut staff.left, &mut staff.right);
		}
	}

	println!("Done");
	staves
}
