use super::*;
use itertools::Itertools;
use gtk::{cairo, gdk, gio, glib, prelude::*};

#[derive(Debug, Clone)]
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

/** Get only the staff bounding boxes, without the surrounding notes */
#[cfg(feature = "editor")]
async fn online_inference(image: &gdk_pixbuf::Pixbuf) -> anyhow::Result<Vec<RelativeStaff>> {
	let image = image.save_to_bufferv("png", &[]).unwrap();
	// let image = &include_bytes!("/home/piegames/Documents/git/OMR-MeasureRecognition/example-images/p001.png")[..];
	let response: serde_json::Value = dbg!(reqwest::Client::new()
		.post("https://inference.piegames.de/dinoscore/upload"
/*"http://localhost:8000/upload"*/)
		.multipart(reqwest::multipart::Form::new()
			.part("file", reqwest::multipart::Part::bytes(image).file_name("file"))
		)
		.send().await?)
		.error_for_status()?
		.json().await?;
	dbg!(&response);
	todo!()
}

/** The code is modeled after a paper from TODO */
#[cfg(feature = "editor")]
pub async fn recognize_staves(image: &gdk_pixbuf::Pixbuf) -> Vec<RelativeStaff> {
	assert!(!image.has_alpha());
	assert!(image.n_channels() == 3);
	assert!(image.colorspace() == gdk_pixbuf::Colorspace::Rgb);

	let image_bytes = image.read_pixel_bytes().unwrap();
	let image_height = image.get_height();
	let image_width = image.get_width();

	let raw_staves = online_inference(image).await;

	// TODO maybe add imageproc dependency? Would it actually help?
	// https://docs.rs/imageproc/latest/imageproc/
	// https://docs.rs/raster/latest/raster/ looks nice too

	todo!();

	// /* Post processing */

	// /* Overlapping is bad */
	// (0..staves.len()).collect::<Vec<_>>()
	// 	.windows(2)
	// 	.for_each(|idx| {
	// 		macro_rules! staff_a (() => {staves[idx[0]]});
	// 		macro_rules! staff_b (() => {staves[idx[1]]});

	// 		if staff_a!().bottom > staff_b!().top
	// 			&& /* 90% horizontal overlap */
	// 			(f64::min(staff_a!().right, staff_b!().right) - f64::max(staff_a!().left, staff_b!().left)) / (f64::max(staff_a!().right, staff_b!().right) - f64::min(staff_a!().left, staff_b!().left)) > 0.9
	// 		{
	// 			let center = (staff_a!().bottom + staff_b!().top) / 2.0;
	// 			staff_a!().bottom = center;
	// 			staff_b!().top = center;
	// 		}
	// 	});

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
	// staves
	todo!()
}

#[cfg(test)]
mod test {
	use super::*;

	#[tokio::test]
	async fn test_inference_api() -> anyhow::Result<()> {
		let bytes = include_bytes!("/home/piegames/Documents/git/OMR-MeasureRecognition/example-images/p001.png");
		let image = gdk_pixbuf::Pixbuf::from_stream(
			&gio::MemoryInputStream::from_bytes(&glib::Bytes::from(bytes as &[u8])),
			Option::<&gio::Cancellable>::None,
		)?;
		let width = 1800;
		let image = image.scale_simple(
			width,
			(width as f64 * image.get_height() / image.get_width()) as i32,
			gdk_pixbuf::InterpType::Bilinear,
		).unwrap();
		online_inference(&image).await?;
		Ok(())
	}
}