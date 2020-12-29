use derive_more::*;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use serde_with::{serde_as, DisplayFromStr};
use std::{
	collections::BTreeMap,
	ops::{Deref, DerefMut, RangeInclusive},
};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct SectionMeta {
	pub is_repetition: bool,
	pub section_end: bool,
}

#[derive(
	Debug,
	Display,
	Serialize,
	Deserialize,
	Clone,
	Copy,
	From,
	FromStr,
	Into,
	AsRef,
	AsMut,
	Deref,
	Add,
	Sub,
	PartialEq,
	Eq,
	PartialOrd,
	Ord,
)]
pub struct StaffIndex(pub usize);

#[derive(
	Debug,
	Display,
	Serialize,
	Deserialize,
	Clone,
	Copy,
	From,
	FromStr,
	Into,
	AsRef,
	AsMut,
	Deref,
	Add,
	Sub,
	PartialEq,
	Eq,
	PartialOrd,
	Ord,
)]
pub struct PageIndex(pub usize);

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(from = "SongMetaVersioned")]
#[serde(into = "SongMetaVersioned")]
#[serde(remote = "Self")]
pub struct SongMeta(pub SongMetaV1);

impl SongMeta {
	pub fn sections(&self) -> Vec<(RangeInclusive<StaffIndex>, bool)> {
		let mut sections = Vec::new();
		let mut iter = self.section_starts.iter().peekable();
		while let Some((key, value)) = iter.next() {
			let start = *key;
			let end = iter
				.peek()
				.map(|(key, value)| {
					if value.section_end {
						**key
					} else {
						**key - 1.into()
					}
				})
				.unwrap_or_else(|| StaffIndex(self.staves.len() - 1));
			sections.push((start..=end, value.is_repetition));
		}
		sections
	}
}

impl<'de> Deserialize<'de> for SongMeta {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		let unchecked = SongMeta::deserialize(deserializer)?;
		if unchecked.staves.is_empty() {
			return Err(de::Error::custom("song must have at least one staff"));
		}
		if !unchecked.piece_starts.contains_key(&0.into()) {
			return Err(de::Error::custom("song must start with a piece"));
		}
		if !unchecked.section_starts.contains_key(&0.into()) {
			return Err(de::Error::custom("song must start with a section"));
		}
		Ok(unchecked)
	}
}

impl Serialize for SongMeta {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		SongMeta::serialize(&self, serializer)
	}
}

impl Deref for SongMeta {
	type Target = SongMetaV1;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl DerefMut for SongMeta {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.0
	}
}

// Remove once https://github.com/serde-rs/serde/issues/1183 is closed
#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SongMetaV1 {
	pub staves: Vec<Line>,
	#[serde_as(as = "BTreeMap<DisplayFromStr, _>")]
	pub piece_starts: BTreeMap<StaffIndex, Option<String>>,
	/// The bool tells if it is a repetition or not
	#[serde_as(as = "BTreeMap<DisplayFromStr, _>")]
	pub section_starts: BTreeMap<StaffIndex, SectionMeta>,
}

// Remove once https://github.com/serde-rs/serde/issues/1183 is closed
#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SongMetaV0 {
	pub staves: Vec<Line>,
	#[serde_as(as = "BTreeMap<DisplayFromStr, _>")]
	pub piece_starts: BTreeMap<StaffIndex, Option<String>>,
	/// The bool tells if it is a repetition or not
	#[serde_as(as = "BTreeMap<DisplayFromStr, _>")]
	pub section_starts: BTreeMap<StaffIndex, bool>,
}

impl Into<SongMetaV1> for SongMetaV0 {
	fn into(self) -> SongMetaV1 {
		SongMetaV1 {
			staves: self.staves,
			piece_starts: self.piece_starts,
			section_starts: self
				.section_starts
				.into_iter()
				.map(|(key, is_repetition)| {
					(
						key,
						SectionMeta {
							is_repetition,
							section_end: false,
						},
					)
				})
				.collect(),
		}
	}
}

impl From<SongMetaVersioned> for SongMeta {
	fn from(versioned: SongMetaVersioned) -> Self {
		match versioned {
			SongMetaVersioned::V1(meta) => SongMeta(meta),
			SongMetaVersioned::V0(meta) => SongMeta(meta.into()),
		}
	}
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "version")]
enum SongMetaVersioned {
	#[serde(rename = "1")]
	V1(SongMetaV1),
	#[serde(rename = "0")]
	V0(SongMetaV0),
}

impl From<SongMeta> for SongMetaVersioned {
	fn from(meta: SongMeta) -> Self {
		SongMetaVersioned::V1(meta.0)
	}
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Line {
	pub page: PageIndex,
	pub start: (f64, f64),
	pub end: (f64, f64),
}

impl Line {
	pub fn get_width(&self) -> f64 {
		self.end.0 - self.start.0
	}

	pub fn get_height(&self) -> f64 {
		self.end.1 - self.start.1
	}
}
