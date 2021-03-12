
use super::*;
use super::collection::*;

use noisy_float::prelude::*;

#[derive(Clone, Debug)]
pub struct StaffLayout {
	pub index: collection::StaffIndex,
	pub x: f64,
	pub y: f64,
	pub width: f64,
}

#[derive(Clone, Debug)]
pub struct PageLayout {
	/* Pages[Staves] */
	pub pages: Vec<Vec<StaffLayout>>,
	/* A random Uuid regenerated for each layout change */
	pub random_id: uuid::Uuid,
}

impl PageLayout {
	/** Get the index of the staff at the center of the page. */
	pub fn get_center_staff(&self, page: PageIndex) -> StaffIndex {
		StaffIndex(
			self.pages[0..*page].iter().map(Vec::len).sum::<usize>() + self.pages[*page].len() / 2,
		)
	}

	pub fn get_staves_of_page(&self, page: PageIndex) -> impl Iterator<Item = StaffIndex> + '_ {
		self.pages[*page].iter().map(|page| page.index)
	}

	pub fn get_page_of_staff(&self, staff: StaffIndex) -> PageIndex {
		let mut sum = 0;
		for (i, page) in self.pages.iter().enumerate() {
			sum += page.len();
			if sum > staff.into() {
				return i.into();
			}
		}
		unreachable!()
	}
}

pub fn layout_fixed_scale(
	song: &collection::SongMeta,
	width: f64,
	height: f64,
	scale: f64,
	pdf_page_width: f64,
) -> PageLayout {
	let scale = scale * height / pdf_page_width;

	/* 1. Find out where the columns of each page start */
	let column_starts = {
		let mut column_starts = Vec::<(StaffIndex, f64)>::new();
		let mut y = 0.0;
		let mut column_width = 0.0;

		for (index, staff) in song.staves.iter().enumerate() {
			let index = StaffIndex(index);
			let staff_width = staff.width() * scale;
			let staff_height = staff.height() * scale;

			column_width = column_width.max(staff_width);

			/* Start a new column for a new piece, or when the page is full */
			if song.piece_starts.contains_key(&index)
				|| (y + staff_height > height)
			{
				y = 0.0;
				column_starts.push((index, column_width));
			}
			y += staff_height;
		}
		/* Without this the last page will get swallowed, because we are iterating over sliding windows */
		column_starts.push((song.staves.len().into(), 0.0));
		column_starts
	};

	/* 2. Convert the start indices to proper start..end ranges */
	/* columns: Vec<(StaffIndex, StaffIndex, f64)> */
	let columns = column_starts 
		.windows(2)
		.map(|v| (v[0], v[1]))
		.map(|((chunk_start, chunk_width), (chunk_end, _))| {
			(chunk_start, chunk_end, chunk_width)
		});

	/* 3. Determine how many columns fit on each page */
	let page_starts: Vec<Vec<(StaffIndex, StaffIndex, f64)>> = {
		let mut pages = Vec::new();
		let mut page = Vec::new();
		let mut x = 0.0;
		for (column_start, column_end, column_width) in columns {
			if (x + column_width > width
			 || song.piece_starts.contains_key(&column_start))
			 && *column_start > 0 {
				pages.push(page);
				page = Vec::new();
				x = 0.0;
			}
			page.push((column_start, column_end, column_width));
			x += column_width;
		}
		pages.push(page);
		pages
	};

	/* 
	 * 4. Calculate the exact position of each staff.
	 * Effectively, this does `(StaffIndex, StaffIndex) -> Vec<StaffLayout>` within our data structure
	 */
	/* pages: Vec<Vec<(Vec<StaffLayout>, f64)>> */
	let pages = page_starts
		.into_iter()
		.map(|page| page
			.into_iter()
			.map(|(column_start, column_end, column_width)| {
				let mut column = Vec::new();
				let staves: &[Staff] = &song.staves[column_start.into()..column_end.into()];

				let staves_total_height = staves
					.iter()
					.map(|staff| staff.height() * scale)
					.sum::<f64>();
				let excess_space = height - staves_total_height;
				/* We limit the spacing to 10% of the average staff height. Thus, it won't spread the staves in
				 * the case there are only a few
				 */
				let max_spacing = staves_total_height / 10.0 / staves.len() as f64;
				let spacing = f64::min(excess_space / staves.len() as f64, max_spacing);
				let mut y = f64::min((excess_space - spacing * (staves.len() - 1) as f64) / 2.0, max_spacing * 3.0);
				for (index, staff) in staves.iter().enumerate() {
					column.push(StaffLayout {
						index: column_start + StaffIndex(index),
						x: (column_width - staff.width() * scale) / 2.0,
						y,
						width: staff.width() * scale,
					});
					y += staff.height() * scale + spacing;
				}

				(column, column_width)
			})
			.collect::<Vec<(Vec<StaffLayout>, f64)>>()
		);

	/*
	 * 5. Merge the multiple columns of each page using iterator magic.
	 * Effectively, this flattens the Vec<(Vec<StaffLayout>, f64)> to a simple Vec<StaffLayout> per page.
	 */
	let pages = pages
		.map(|columns| {
			let excess_space = width
			- columns
				.iter()
				.map(|(_column, column_width)| column_width)
				.sum::<f64>();
			let spacing = excess_space / columns.len() as f64;
			let mut x = (excess_space - spacing * (columns.len() - 1) as f64) / 2.0;

			columns
				.into_iter()
				.enumerate()
				.flat_map(|(index, (column, column_width))| {
					let old_x = x;
					x += column_width + spacing;

					column.into_iter().map(move |staff| StaffLayout {
						index: staff.index,
						x: staff.x + old_x,
						y: staff.y,
						width: staff.width,
					})
				})
				.collect()
		})
		.collect();

	PageLayout {
		pages,
		random_id: uuid::Uuid::new_v4(),
	}
}

pub fn layout_fixed_width(
	song: &collection::SongMeta,
	width: f64,
	height: f64,
	zoom: f64,
	column_count: usize,
	spacing: f64,
) -> PageLayout {
	let column_width = (width / column_count as f64) * zoom;
	/* 1. Segment the staves to fit onto columns */
	let column_starts = {
		let mut column_starts = Vec::<StaffIndex>::new();
		let mut y = 0.0;
		// let mut y_with_spacing = 0.0;
		for (index, staff) in song.staves.iter().enumerate() {
			let index = StaffIndex(index);

			let staff_height = column_width * staff.aspect_ratio();

			/*
			 * Start a new column for a new piece.
			 * If staff doesn't fit anymore, first try to squeeze it in at the cost of spacing
			 */
			if song.piece_starts.contains_key(&index)
				// || ((y_with_spacing + staff_height > height) && (y + staff_height <= height))
				|| (y + staff_height > height)
			{
				y = 0.0;
				// y_with_spacing = 0.0;
				column_starts.push(index);
			}
			y += staff_height;
			// y_with_spacing += staff_height + spacing;
		}
		/* Without this the last page will get swallowed */
		column_starts.push(song.staves.len().into());
		column_starts
	};

	/* 2. Calculate the exact position of each staff */
	let columns: Vec<Vec<StaffLayout>> = column_starts
		.windows(2)
		.map(|v| (v[0], v[1]))
		.map(|(chunk_start, chunk_end)| {
			let mut column = Vec::new();
			let staves: &[Staff] = &song.staves[chunk_start.into()..chunk_end.into()];
			if staves.len() == 1 {
				let staff = &staves[0];
				let staff_height = column_width * staff.aspect_ratio();
				let x;
				let y;
				let staff_width;

				if staff_height > height {
					staff_width = height / staff.aspect_ratio();
					x = (column_width - staff_width) / 2.0;
					y = 0.0;
				} else {
					staff_width = column_width;
					x = 0.0;
					y = (height - staff_height) / 2.0;
				}

				column.push(StaffLayout {
					index: chunk_start,
					x,
					y,
					width: staff_width,
				});
			} else {
				let excess_space = height
					- staves
						.iter()
						.map(|staff| column_width * staff.aspect_ratio())
						.sum::<f64>();
				let spacing = f64::min(spacing, excess_space / staves.len() as f64);
				let mut y = (excess_space - spacing * staves.len() as f64) / 2.0;
				for (index, staff) in staves.iter().enumerate() {
					column.push(StaffLayout {
						index: chunk_start + StaffIndex(index),
						x: 0.0,
						y,
						width: column_width,
					});
					y += column_width * staff.aspect_ratio() + spacing;
				}
			}
			column
		})
		.collect();

	/* 3. Merge the single columns to pages using iterator magic */
	let left_margin = (width - width * zoom) / 2.0;
	let pages = columns
		.chunks(column_count)
		.map(|chunk| {
			chunk
				.iter()
				.enumerate()
				.flat_map(|(i, c)| {
					c.iter().map(move |staff| StaffLayout {
						index: staff.index,
						x: staff.x + column_width * (i % column_count) as f64 + left_margin,
						y: staff.y,
						width: staff.width,
					})
				})
				.collect()
		})
		.collect();

	PageLayout {
		pages,
		random_id: uuid::Uuid::new_v4(),
	}
}

pub fn layout_fixed_height(song: &collection::SongMeta, width: f64, height: f64, row_count: usize) -> PageLayout {
	let row_height = height / row_count as f64;

	let column_starts = {
		let mut column_starts = Vec::<StaffIndex>::new();
		let mut page_length = 0;
		for index in 0..song.staves.len() {
			let index = StaffIndex(index);

			if song.piece_starts.contains_key(&index) || page_length >= row_count {
				column_starts.push(index);
				page_length = 0;
			}
			page_length += 1;
		}
		column_starts.push(song.staves.len().into());
		column_starts
	};

	let pages = column_starts
		.windows(2)
		.map(|v| (v[0], v[1]))
		.map(|(chunk_start, chunk_end)| {
			let staves: &[Staff] = &song.staves[chunk_start.into()..chunk_end.into()];
			let max_width: f64 = staves
				.iter()
				.map(|staff| r64(row_height / staff.aspect_ratio()))
				.min() /* min is correct here */
				.expect("Page cannot be empty")
				.into();
			let max_width = max_width.min(width);

			staves
				.iter()
				.enumerate()
				.map(|(in_page_index, staff)| {
					let mut staff_width = row_height / staff.aspect_ratio();
					let mut staff_height = row_height;

					if staff_width > max_width {
						staff_width = max_width;
						staff_height *= staff_width / max_width;
					}

					StaffLayout {
						index: StaffIndex(in_page_index) + chunk_start,
						x: (width - staff_width) / 2.0,
						y: in_page_index as f64 * row_height
							+ (row_height - staff_height) / 2.0,
						width: staff_width,
					}
				})
				.collect::<Vec<_>>()
		})
		.collect::<Vec<_>>();

	PageLayout {
		pages,
		random_id: uuid::Uuid::new_v4(),
	}
}
