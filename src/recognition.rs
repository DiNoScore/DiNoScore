use super::*;
use gtk::{cairo, gdk, gdk_pixbuf, gio, glib, prelude::*};
use itertools::Itertools;

/* Origin top left corner */
#[derive(Debug, Copy, Clone)]
pub struct RelativeStaff {
	pub left: f64,
	pub top: f64,
	pub right: f64,
	pub bottom: f64,
}

impl RelativeStaff {
	pub fn into_staff(
		self,
		page: collection::PageIndex,
		width: f64,
		height: f64,
	) -> collection::Staff {
		collection::Staff {
			page,
			start: (self.left * width, self.top * height),
			end: (self.right * width, self.bottom * height),
		}
	}
}

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
async fn online_inference(image: &image::GrayImage) -> anyhow::Result<Vec<AbsoluteStaff>> {
	let mut png = Vec::with_capacity(8096);
	image::write_buffer_with_format(
		&mut std::io::Cursor::new(&mut png),
		image.as_raw(),
		image.width(),
		image.height(),
		image::ColorType::L8,
		image::ImageOutputFormat::Png,
	)?;
	let response: serde_json::Value = reqwest::Client::new()
		.post("https://inference.piegames.de/dinoscore/upload")
		// .post("http://localhost:8000/upload")
		.multipart(reqwest::multipart::Form::new().part(
			"file",
			reqwest::multipart::Part::bytes(png).file_name("file"),
		))
		.send()
		.await?
		.error_for_status()?
		.json()
		.await?;
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

/** The code is modeled after a paper from TODO */
#[cfg(feature = "editor")]
pub async fn recognize_staves(image: &gdk_pixbuf::Pixbuf) -> Vec<RelativeStaff> {
	let png = image.save_to_bufferv("png", &[]).unwrap();
	let image: image::GrayImage = image::load_from_memory(&png).unwrap().into_luma8();
	let image_width = image.width();
	let image_height = image.height();

	let mut raw_staves = online_inference(&image).await.unwrap();
	// return raw_staves.unwrap();
	// TODO maybe add imageproc dependency? Would it actually help?
	// https://docs.rs/imageproc/latest/imageproc/

	/* Compute the integral once, and then use it to query arbitrary sub-rectangles */
	let integral_image = imageproc::integral_image::integral_image(&image);
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
					staff.left + window_width,
					staff.bottom,
				)[0];
				/* Check for the average brightness in the window. Break if less than 2% content */
				let average = sum as f32 / (window_size as f32 * 255.0);
				if staff.left == 0 || average >= 0.98 {
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
					staff.right,
					staff.bottom,
				)[0];
				/* Check for the average brightness in the window. Break if less than 2% content */
				let average = sum as f32 / (window_size as f32 * 255.0);
				if staff.right == image_width || average >= 0.98 {
					break;
				}
				staff.right += 1;
			}
		}
	}

	/* Sort them from top to bottom. Also remove overlap */
	raw_staves.sort_by_key(|staff| staff.top);
	(0..raw_staves.len()).collect::<Vec<_>>()
		.windows(2)
		.for_each(|idx| {
			macro_rules! staff_a (() => {raw_staves[idx[0]]});
			macro_rules! staff_b (() => {raw_staves[idx[1]]});

			if staff_a!().bottom > staff_b!().top
				&& /* 90% horizontal overlap */
				100 * (staff_a!().right.min(staff_b!().right) - staff_a!().left.max(staff_b!().left)) / (staff_a!().right.max(staff_b!().right) - staff_a!().left.min(staff_b!().left)) > 90
			{
				let center = (staff_a!().bottom + staff_b!().top) / 2;
				staff_a!().bottom = center;
				staff_b!().top = center;
			}
		});

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
				if 100 * (staff_a.right.min(staff_b.right) - staff_a.left.max(staff_b.left))
					/ (staff_a.right.max(staff_b.right) - staff_a.left.min(staff_b.left))
					<= 90
				{
					return false;
				}

				/* Limit distance they can be apart */
				if staff_b.bottom - staff_a.top > 3 * (staff_a.height() + staff_b.height()) {
					return false;
				}

				/* Search for a strong vertical connection (size 2% of width) at the beginning (in the first 5%) */
				let window_width = (staff_a.width() / 50).max(4);
				let window_size = window_width * (staff_b.bottom - staff_a.top);
				for x in staff_a.left.min(staff_b.left)
					..staff_a.left.max(staff_b.left) + staff_a.width() / 20
				{
					let sum: u32 = imageproc::integral_image::sum_image_pixels(
						&integral_image,
						x,
						staff_a.top,
						x + window_width,
						staff_b.bottom,
					)[0];
					/* Check for the average brightness in the window. We want at least 20% content */
					let average = sum as f32 / (window_size as f32 * 255.0);
					if average < 0.8 {
						return true;
					}
				}

				false
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

	/* Extend staves up and down to content */
	{
		for staff in &mut raw_staves {
			let window_width = (staff.width() / 50).max(4);
			let window_height = (staff.height() / 20).max(4);
			let window_size = window_width * window_height;

			for _ in 0..3 {
				for x in staff.left..staff.right - window_width {
					/* Down */
					loop {
						let sum: u32 = imageproc::integral_image::sum_image_pixels(
							&integral_image,
							x,
							staff.bottom - window_height,
							x + window_width,
							staff.bottom,
						)[0];
						let average = sum as f32 / (window_size as f32 * 255.0);
						if average < 0.97 && staff.bottom < image_height {
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
							x + window_width,
							staff.top + window_height,
						)[0];
						let average = sum as f32 / (window_size as f32 * 255.0);
						if average < 0.97 && staff.top > 0 {
							staff.top -= 1
						} else {
							break;
						}
					}
				}
			}
		}

		/* Clip overlaps again */
		// (0..raw_staves.len()).collect::<Vec<_>>()
		// 	.windows(2)
		// 	.for_each(|idx| {
		// 		macro_rules! staff_a (() => {raw_staves[idx[0]]});
		// 		macro_rules! staff_b (() => {raw_staves[idx[1]]});

		// 		if staff_a!().bottom > staff_b!().top
		// 			&& /* 90% horizontal overlap */
		// 			100 * (staff_a!().right.min(staff_b!().right) - staff_a!().left.max(staff_b!().left)) / (staff_a!().right.max(staff_b!().right) - staff_a!().left.min(staff_b!().left)) > 90
		// 		{
		// 			let center = (staff_a!().bottom + staff_b!().top) / 2;
		// 			staff_a!().bottom = center;
		// 			staff_b!().top = center;
		// 		}
		// 	});
	}

	/* Convert back to relative positions. Filter for too small artefacts */
	let staves = raw_staves
		.iter()
		.map(|staff| RelativeStaff {
			top: staff.top as f64 / image_height as f64,
			bottom: staff.bottom as f64 / image_height as f64,
			left: staff.left as f64 / image_width as f64,
			right: staff.right as f64 / image_width as f64,
		})
		/* At least 20% width */
		.filter(|staff| staff.right - staff.left >= 0.2)
		/* At least 1% height */
		.filter(|staff| staff.bottom - staff.top >= 0.01)
		.collect::<Vec<_>>();

	// /* Post processing */
	// /* Overlapping is bad */
	// /* Fixup fuckups */
	// for staff in &mut staves {
	// 	if staff.top > staff.bottom {
	// 		std::mem::swap(&mut staff.top, &mut staff.bottom);
	// 	}
	// 	if staff.left > staff.right {
	// 		std::mem::swap(&mut staff.left, &mut staff.right);
	// 	}
	// }

	// log::debug!("Done");
	staves
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
}
