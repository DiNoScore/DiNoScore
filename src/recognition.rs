use super::*;
use gtk::{cairo, gdk, gdk_pixbuf, gio, glib, prelude::*};
use itertools::Itertools;
use typed_index_collections::TiVec;

#[derive(serde::Deserialize, Debug, Clone)]
struct Response {
	width: u32,
	height: u32,
	// #[serde(alias="system_measures", alias="stave_measures")]
	staves: Vec<AbsoluteStaff>,
}

/* Origin top left corner */
#[derive(serde::Deserialize, Debug, Copy, Clone)]
struct AbsoluteStaff {
	bottom: u32,
	left: u32,
	top: u32,
	right: u32,
}

impl AbsoluteStaff {
	fn width(&self) -> u32 {
		self.right - self.left
	}

	fn height(&self) -> u32 {
		self.bottom - self.top
	}
}

/** Get only the staff bounding boxes, without the surrounding notes */
#[cfg(feature = "editor")]
fn online_inference(image: &image::GrayImage) -> anyhow::Result<Vec<AbsoluteStaff>> {
	let mut png = Vec::with_capacity(8096);
	image::write_buffer_with_format(
		&mut std::io::Cursor::new(&mut png),
		image.as_raw(),
		image.width(),
		image.height(),
		image::ColorType::L8,
		image::ImageOutputFormat::Png,
	)?;
	let response: serde_json::Value = attohttpc:://post("https://inference.piegames.de/dinoscore/upload")
			post("http://localhost:8000/upload")
	.body(
		attohttpc::MultipartBuilder::new()
			.with_file(attohttpc::MultipartFile::new("file", &png).with_filename("file"))
			.build()?,
	)
	.send()?
	.error_for_status()?
	.json()?;
	let response: Vec<Response> = serde_json::from_value(response).unwrap();
	let response = &response[0];
	Ok(response.staves.clone())
	// Ok(response.staves.iter()
	// 	.map(|staff| RelativeStaff {
	// 		top: staff.top as f64 / response.height as f64,
	// 		bottom: staff.bottom as f64 / response.height as f64,
	// 		left: staff.left as f64 / response.width as f64,
	// 		right: staff.right as f64 / response.width as f64,
	// 	})
	// 	.collect()
	// )
}

#[cfg(feature = "editor")]
fn post_process(
	mut raw_staves: Vec<AbsoluteStaff>,
	image: &image::GrayImage,
	page: collection::PageIndex,
) -> Vec<collection::Staff> {
	if raw_staves.len() == 0 {
		return vec![];
	}

	use image::GenericImageView;

	let image_width = image.width();
	let image_height = image.height();

	/* Debugging: return unprocessed staves */
	if false {
		// println!("{}", serde_json::to_string_pretty(&
		return raw_staves
			.iter()
			.map(|staff| collection::Staff {
				page,
				start: (
					staff.left as f64 / image_width as f64,
					staff.top as f64 / image_width as f64,
				),
				end: (
					staff.right as f64 / image_width as f64,
					staff.bottom as f64 / image_width as f64,
				),
			})
			.collect::<Vec<_>>();
		// ).unwrap());
	}

	/* Sanitize input; clamp to image size */
	for staff in &mut raw_staves {
		staff.right = staff.right.min(image_width - 1);
		staff.bottom = staff.bottom.min(image_height - 1);
	}

	/* Compute the integral once, and then use it to query arbitrary sub-rectangles */
	let integral_image = imageproc::integral_image::integral_image::<_, u32>(&image);
	// /* Blur out horizontal components (1%) */
	// let blurred_image = imageproc::filter::box_filter(&image, 0, image_height / 100);

	/* Extend staves to the left and right until no more content */
	{
		for staff in &mut raw_staves {
			/* Check a sliding window 2% of the staff width for content */
			let window_width = (staff.width() / 50).max(4);
			let window_size = window_width * staff.height();

			/* Extend to left */
			loop {
				let sum: u32 = imageproc::integral_image::sum_image_pixels(
					&integral_image,
					staff.left,
					staff.top,
					staff.left + window_width - 1,
					staff.bottom - 1,
				)[0];
				/* Check for the average brightness in the window. Break if less than 1% content */
				let average = sum as f32 / (window_size as f32 * 255.0);
				if staff.left == 0 || average >= 0.99 {
					break;
				}
				staff.left -= 1;
			}
			/* Extend to right */
			loop {
				let sum: u32 = imageproc::integral_image::sum_image_pixels(
					&integral_image,
					staff.right - window_width,
					staff.top,
					staff.right - 1,
					staff.bottom - 1,
				)[0];
				/* Check for the average brightness in the window. Break if less than 1% content */
				let average = sum as f32 / (window_size as f32 * 255.0);
				if staff.right == image_width - 1 || average >= 0.99 {
					break;
				}
				staff.right += 1;
			}
		}
	}

	/* 90% horizontal overlap */
	fn overlaps(a_left: u32, a_right: u32, b_left: u32, b_right: u32) -> bool {
		/* No overlap at all */
		if a_right < b_left || a_left > b_right {
			return false;
		}
		let small = u32::saturating_sub(a_right.min(b_right), a_left.max(b_left));
		let big = u32::saturating_sub(a_right.max(b_right), a_left.min(b_left));
		if small == 0 || big == 0 {
			return false;
		}
		100 * small / big > 90
	}

	/* Sort them from top to bottom. Also remove overlap */
	// TODO we don't really care about overlap here, and there should not be any regardless.
	raw_staves.sort_by_key(|staff| staff.top);
	(0..raw_staves.len())
		.collect::<Vec<_>>()
		.windows(2)
		.for_each(|idx| {
			macro_rules! staff_a (() => {raw_staves[idx[0]]});
			macro_rules! staff_b (() => {raw_staves[idx[1]]});

			if staff_a!().bottom > staff_b!().top
				&& /* 90% horizontal overlap */
				overlaps(staff_a!().left, staff_a!().right, staff_b!().left, staff_b!().right)
			{
				let center = (staff_a!().bottom + staff_b!().top) / 2;
				staff_a!().bottom = center;
				staff_b!().top = center;
			}
		});

	/* Double all staves in height (expand 50% up and down). This is important because it
	 * has an impact on overlap clipping later on. We need to do it before merging though,
	 * to be invariant across multi-staff systems.
	 */
	for staff in &mut raw_staves {
		let height = staff.height() / 2;
		staff.top = staff.top.max(height) - height;
		staff.bottom = (staff.bottom + height).min(image_height - 1);
	}

	/* Merge staves from the same system */
	{
		let to_merge = (0..raw_staves.len())
			.collect::<Vec<_>>()
			.windows(2)
			.rev()
			.map(|idx| (idx[0], idx[1]))
			.filter(|(staff_a, staff_b)| {
				let staff_a = &raw_staves[*staff_a];
				let staff_b = &raw_staves[*staff_b];

				/* Need 90% horizontal overlap */
				if !overlaps(staff_a.left, staff_a.right, staff_b.left, staff_b.right) {
					return false;
				}

				/* Limit distance they can be apart */
				if staff_b.bottom - staff_a.top > 3 * (staff_a.height() + staff_b.height()) {
					return false;
				}

				/* Search for a vertical connection between both near the start of the staff */

				/* Search within the first 5% of the staff, on the full height */
				let window_width = (staff_a.width() / 20).max(4);
				let window_height = staff_b.bottom - staff_a.top;
				let connected_image = pipeline::pipe!(
					image.view(staff_a.left.min(staff_b.left), staff_a.top, window_width, window_height)
					=> _.to_image()
					=> imageproc::filter::box_filter(&_, 2, 2)
					=> (|mut img| { imageproc::contrast::threshold_mut(&mut img, 250); img })
					=> imageproc::region_labelling::connected_components(&_, imageproc::region_labelling::Connectivity::Eight, [255].into())
				);

				/* Map the content of the top and bottom staff to the components, intersect */
				let components_a = connected_image.view(0, 0, window_width, staff_a.height())
					.pixels()
					.map(|(_x, _y, v)| v.0[0])
					.filter(|&val| val != 0)
					.collect::<HashSet<u32>>();
				let components_b = connected_image.view(0, window_height - staff_b.height(), window_width, staff_b.height())
					.pixels()
					.map(|(_x, _y, v)| v.0[0])
					.filter(|&val| val != 0)
					.collect::<HashSet<u32>>();

				components_a.intersection(&components_b).next().is_some()
			})
			.collect::<Vec<_>>();

		for (staff_a, staff_b) in to_merge {
			let staff_b = raw_staves.remove(staff_b);
			let staff_a = &mut raw_staves[staff_a];
			staff_a.left = staff_a.left.min(staff_b.left);
			staff_a.top = staff_a.top.min(staff_b.top);
			staff_a.right = staff_a.right.max(staff_b.right);
			staff_a.bottom = staff_a.bottom.max(staff_b.bottom);
		}
	}

	/* Extend staves up and down to content, but make sure not to overlap with above and below */
	{
		/* Make a backup. This will be used as a reference for clipping overlaps later on */
		let backup_staves = raw_staves.clone();

		/* First of all, include the directly connected component */

		let connected_image = pipeline::pipe!(
			imageproc::filter::box_filter(&image, 2, 2)
			=> (|mut img| { imageproc::contrast::threshold_mut(&mut img, 220); img })
			=> imageproc::region_labelling::connected_components(&_, imageproc::region_labelling::Connectivity::Eight, [255].into())
		);

		for staff in &mut raw_staves {
			let components = connected_image
				.view(staff.left, staff.top, staff.width(), staff.height())
				.pixels()
				.map(|(_x, _y, v)| v.0[0])
				.filter(|&val| val != 0)
				.collect::<HashSet<u32>>();

			/* Follow connected component down */
			let mut new_bottom = staff.bottom;
			'outer1: for y in staff.bottom..(staff.bottom + staff.height()).min(image_height) {
				for x in staff.left..staff.right {
					if components.contains(&connected_image.get_pixel(x, y).0[0]) {
						new_bottom = y;
						continue 'outer1;
					}
				}
				break;
			}

			/* Follow connected component up */
			let mut new_top = staff.top;
			'outer2: for y in (staff.top - (staff.height().min(staff.top))..staff.top).rev() {
				for x in staff.left..staff.right {
					if components.contains(&connected_image.get_pixel(x, y).0[0]) {
						new_top = y;
						continue 'outer2;
					}
				}
				break;
			}

			staff.bottom = new_bottom;
			staff.top = new_top;
		}

		/* Then, expand based on close-by content. */
		for staff in &mut raw_staves {
			let window_width = (staff.width() / 80).max(4);
			let window_height = (staff.height() / 80).max(4);
			let window_size = window_width * window_height;

			/* Iterate multiple times */
			for _ in 0..1 {
				// previously: 0..3
				/* Traverse the the staff horizontally with a sliding window */
				for x in staff.left..staff.right - window_width {
					/* Down */
					loop {
						let sum: u32 = imageproc::integral_image::sum_image_pixels(
							&integral_image,
							x,
							staff.bottom - window_height,
							x + window_width - 1,
							staff.bottom - 1,
						)[0];
						let average = sum as f32 / (window_size as f32 * 255.0);
						if average < 0.92 && staff.bottom < image_height - 1 {
							staff.bottom += 1
						} else {
							break;
						}
					}
					/* Up */
					loop {
						let sum: u32 = imageproc::integral_image::sum_image_pixels(
							&integral_image,
							x,
							staff.top,
							x + window_width - 1,
							staff.top + window_height - 1,
						)[0];
						let average = sum as f32 / (window_size as f32 * 255.0);
						if average < 0.92 && staff.top > 0 {
							staff.top -= 1
						} else {
							break;
						}
					}
				}
			}
		}

		/* Clipping of too much overlap */
		for idx in 0..raw_staves.len() - 1 {
			let (a, b) = (idx, idx + 1);

			/* Clamp to the backup as reference */
			raw_staves[a].bottom = raw_staves[a].bottom.min(backup_staves[b].top);
			raw_staves[b].top = raw_staves[b].top.max(backup_staves[a].bottom);

			/* If there is overlap but we can find a straight empty horizontal line then use that as separator */
			if raw_staves[a].bottom > raw_staves[b].top {
				/* If we find multiple such positions, then use the outermost ones */
				let mut lines = Vec::new();
				let left = raw_staves[a].left.min(raw_staves[b].left);
				let right = raw_staves[a].right.max(raw_staves[b].right);
				for y in raw_staves[b].top..raw_staves[a].bottom {
					let sum: u32 = imageproc::integral_image::sum_image_pixels(
						&integral_image,
						left,
						(y - 1).max(0),
						right,
						(y + 1).min(image_height - 1),
					)[0];
					let average = sum as f64 / (right - left) as f64 / 3.0 / 255.0;
					if average > 0.9995 {
						lines.push(y);
					}
				}
				match &*lines {
					[] => {},
					[y] => {
						raw_staves[a].bottom = *y;
						raw_staves[b].top = *y;
					},
					/* Multiple candidates: use the first and last one, respectively */
					[y1, .., y2] => {
						raw_staves[a].bottom = *y1;
						raw_staves[b].top = *y2;
					},
				}
			}
		}
	}

	/* Convert back to relative positions. Filter for too small artefacts */
	let staves = raw_staves
		.iter()
		.map(|staff| collection::Staff {
			page,
			start: (
				staff.left as f64 / image_width as f64,
				staff.top as f64 / image_width as f64,
			),
			end: (
				staff.right as f64 / image_width as f64,
				staff.bottom as f64 / image_width as f64,
			),
		})
		/* At least 20% width */
		.filter(|staff| staff.width() >= 0.2)
		/* At least 1% height */
		.filter(|staff| staff.height() >= 0.01)
		.collect::<Vec<_>>();

	staves
}

#[cfg(feature = "editor")]
pub fn recognize_staves(
	image: &gdk_pixbuf::Pixbuf,
	page: collection::PageIndex,
) -> Vec<collection::Staff> {
	let png = image.save_to_bufferv("png", &[]).unwrap();
	let image: image::GrayImage = image::load_from_memory(&png).unwrap().into_luma8();

	/* For manual debugging only: replace inference with local results to avoid doing it over and over again */
	let raw_staves: Vec<AbsoluteStaff> = if cfg!(any()) {
		#[derive(serde::Deserialize)]
		struct ReferenceData {
			staves_per_page: TiVec<collection::PageIndex, usize>,
			raw_staves: TiVec<collection::PageIndex, Vec<AbsoluteStaff>>,
		}
		serde_json::from_slice::<std::collections::BTreeMap<String, ReferenceData>>(
			&*include_bytes!("../test/recognition/reference_data.json"),
		)
		.unwrap()
		.into_values()
		.flat_map(|value| value.raw_staves)
		.nth(*page)
		.unwrap()
	} else {
		online_inference(&image).unwrap()
	};

	post_process(raw_staves, &image, page)
}

#[cfg(test)]
mod test {
	use super::*;

	// #[tokio::test]
	// async fn test_inference_api() -> anyhow::Result<()> {
	// 	let image = gdk_pixbuf::Pixbuf::from_stream(
	// 		&gio::MemoryInputStream::from_bytes(&glib::Bytes::from(bytes as &[u8])),
	// 		Option::<&gio::Cancellable>::None,
	// 	)?;
	// 	let width = 1800;
	// 	let image = image.scale_simple(
	// 		width,
	// 		(width as f64 * image.get_height() / image.get_width()) as i32,
	// 		gdk_pixbuf::InterpType::Bilinear,
	// 	).unwrap();
	// 	online_inference(&image).await?;
	// 	Ok(())
	// }

	#[derive(serde::Deserialize)]
	struct ReferenceData {
		staves_per_page: TiVec<collection::PageIndex, usize>,
		raw_staves: TiVec<collection::PageIndex, Vec<AbsoluteStaff>>,
	}

	#[test]
	fn test_post_processing() {
		let reference_data: HashMap<String, ReferenceData> =
			serde_json::from_slice(&*include_bytes!("../test/recognition/reference_data.json"))
				.unwrap();

		for (name, reference_data) in &reference_data {
			let pdf: Vec<Vec<u8>> = pipeline::pipe!(
				std::fs::read(format!("test/recognition/{name}.pdf")).unwrap()
				=> image_util::explode_pdf_raw(&_).unwrap()
			);
			for (page, (index, raw_staves)) in pdf
				.into_iter()
				.zip(reference_data.raw_staves.iter_enumerated())
			{
				let image: image::GrayImage = pipeline::pipe!(
					page
					=> image_util::PageImage::from_pdf
					=> Result::unwrap
					=> _.render_scaled(400)
					=> _.save_to_bufferv("png", &[]).unwrap()
					=> image::load_from_memory(&_).unwrap().into_luma8()
				);

				let processed = post_process(raw_staves.clone(), &image, index);
				println!(
					"{name} {index} {} {} {}",
					raw_staves.len(),
					processed.len(),
					reference_data.staves_per_page[index]
				);
				assert_eq!(
					processed.len(),
					reference_data.staves_per_page[index],
					"Invalid number of staves found! File {name}, page {index}"
				);
			}
		}
	}

	/// Post-processing tends to panic on bounds checks (:
	#[test]
	fn test_edges() {
		let image: image::GrayImage = pipeline::pipe!(
			std::fs::read(format!("test/recognition/edges.tif")).unwrap()
			=> image_util::PageImage::from_image(_, "tif".into()).unwrap()
			=> _.render_scaled(400)
			=> _.save_to_bufferv("png", &[]).unwrap()
			=> image::load_from_memory(&_).unwrap().into_luma8()
		);

		/* Those are actually slightly wrong, but that's all we need to reproduce the bug */
		let raw_staves = vec![
			AbsoluteStaff {
				left: 31,
				top: 41,
				right: 369,
				bottom: 155,
			},
			AbsoluteStaff {
				left: 33,
				top: 159,
				right: 370,
				bottom: 276,
			},
			AbsoluteStaff {
				left: 31,
				top: 276,
				right: 371,
				bottom: 393,
			},
			AbsoluteStaff {
				left: 34,
				top: 393,
				right: 370,
				bottom: 508,
			},
		];

		post_process(raw_staves, &image, collection::PageIndex(1));

		/* A different set of staves that crashes, just for fun */
		let raw_staves = vec![
			AbsoluteStaff {
				bottom: 507,
				left: 8,
				top: 489,
				right: 400,
			},
			AbsoluteStaff {
				bottom: 231,
				left: 7,
				top: 213,
				right: 394,
			},
			AbsoluteStaff {
				bottom: 92,
				left: 7,
				top: 74,
				right: 397,
			},
			AbsoluteStaff {
				bottom: 416,
				left: 8,
				top: 398,
				right: 400,
			},
			AbsoluteStaff {
				bottom: 552,
				left: 7,
				top: 534,
				right: 400,
			},
			AbsoluteStaff {
				bottom: 132,
				left: 16,
				top: 114,
				right: 394,
			},
			AbsoluteStaff {
				bottom: 371,
				left: 9,
				top: 353,
				right: 400,
			},
			AbsoluteStaff {
				bottom: 276,
				left: 9,
				top: 258,
				right: 400,
			},
			AbsoluteStaff {
				bottom: 181,
				left: 9,
				top: 165,
				right: 387,
			},
			AbsoluteStaff {
				bottom: 40,
				left: 8,
				top: 26,
				right: 386,
			},
			AbsoluteStaff {
				bottom: 324,
				left: 9,
				top: 308,
				right: 400,
			},
			AbsoluteStaff {
				bottom: 463,
				left: 9,
				top: 448,
				right: 400,
			},
		];

		post_process(raw_staves, &image, collection::PageIndex(1));
	}
}
