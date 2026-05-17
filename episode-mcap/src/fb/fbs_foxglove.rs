// AUTO-MERGED by tools/merge_fbs.py — DO NOT EDIT BY HAND.
// Inputs: src/fbs/*_generated.rs (verbatim flatc output)
// Namespace: foxglove


extern crate alloc;

pub mod foxglove {

// ===== from ArrowPrimitive_generated.rs =====



pub enum ArrowPrimitiveOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A primitive representing an arrow
pub struct ArrowPrimitive<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for ArrowPrimitive<'a> {
  type Inner = ArrowPrimitive<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> ArrowPrimitive<'a> {
  pub const VT_POSE: ::flatbuffers::VOffsetT = 4;
  pub const VT_SHAFT_LENGTH: ::flatbuffers::VOffsetT = 6;
  pub const VT_SHAFT_DIAMETER: ::flatbuffers::VOffsetT = 8;
  pub const VT_HEAD_LENGTH: ::flatbuffers::VOffsetT = 10;
  pub const VT_HEAD_DIAMETER: ::flatbuffers::VOffsetT = 12;
  pub const VT_COLOR: ::flatbuffers::VOffsetT = 14;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    ArrowPrimitive { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args ArrowPrimitiveArgs<'args>
  ) -> ::flatbuffers::WIPOffset<ArrowPrimitive<'bldr>> {
    let mut builder = ArrowPrimitiveBuilder::new(_fbb);
    builder.add_head_diameter(args.head_diameter);
    builder.add_head_length(args.head_length);
    builder.add_shaft_diameter(args.shaft_diameter);
    builder.add_shaft_length(args.shaft_length);
    if let Some(x) = args.color { builder.add_color(x); }
    if let Some(x) = args.pose { builder.add_pose(x); }
    builder.finish()
  }


  /// Position of the arrow's tail and orientation of the arrow. Identity orientation means the arrow points in the +x direction.
  #[inline]
  pub fn pose(&self) -> Option<Pose<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Pose>>(ArrowPrimitive::VT_POSE, None)}
  }
  /// Length of the arrow shaft
  #[inline]
  pub fn shaft_length(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(ArrowPrimitive::VT_SHAFT_LENGTH, Some(0.0)).unwrap()}
  }
  /// Diameter of the arrow shaft
  #[inline]
  pub fn shaft_diameter(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(ArrowPrimitive::VT_SHAFT_DIAMETER, Some(0.0)).unwrap()}
  }
  /// Length of the arrow head
  #[inline]
  pub fn head_length(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(ArrowPrimitive::VT_HEAD_LENGTH, Some(0.0)).unwrap()}
  }
  /// Diameter of the arrow head
  #[inline]
  pub fn head_diameter(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(ArrowPrimitive::VT_HEAD_DIAMETER, Some(0.0)).unwrap()}
  }
  /// Color of the arrow
  #[inline]
  pub fn color(&self) -> Option<Color<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Color>>(ArrowPrimitive::VT_COLOR, None)}
  }
}

impl ::flatbuffers::Verifiable for ArrowPrimitive<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Pose>>("pose", Self::VT_POSE, false)?
     .visit_field::<f64>("shaft_length", Self::VT_SHAFT_LENGTH, false)?
     .visit_field::<f64>("shaft_diameter", Self::VT_SHAFT_DIAMETER, false)?
     .visit_field::<f64>("head_length", Self::VT_HEAD_LENGTH, false)?
     .visit_field::<f64>("head_diameter", Self::VT_HEAD_DIAMETER, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Color>>("color", Self::VT_COLOR, false)?
     .finish();
    Ok(())
  }
}
pub struct ArrowPrimitiveArgs<'a> {
    pub pose: Option<::flatbuffers::WIPOffset<Pose<'a>>>,
    pub shaft_length: f64,
    pub shaft_diameter: f64,
    pub head_length: f64,
    pub head_diameter: f64,
    pub color: Option<::flatbuffers::WIPOffset<Color<'a>>>,
}
impl<'a> Default for ArrowPrimitiveArgs<'a> {
  #[inline]
  fn default() -> Self {
    ArrowPrimitiveArgs {
      pose: None,
      shaft_length: 0.0,
      shaft_diameter: 0.0,
      head_length: 0.0,
      head_diameter: 0.0,
      color: None,
    }
  }
}

pub struct ArrowPrimitiveBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> ArrowPrimitiveBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_pose(&mut self, pose: ::flatbuffers::WIPOffset<Pose<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Pose>>(ArrowPrimitive::VT_POSE, pose);
  }
  #[inline]
  pub fn add_shaft_length(&mut self, shaft_length: f64) {
    self.fbb_.push_slot::<f64>(ArrowPrimitive::VT_SHAFT_LENGTH, shaft_length, 0.0);
  }
  #[inline]
  pub fn add_shaft_diameter(&mut self, shaft_diameter: f64) {
    self.fbb_.push_slot::<f64>(ArrowPrimitive::VT_SHAFT_DIAMETER, shaft_diameter, 0.0);
  }
  #[inline]
  pub fn add_head_length(&mut self, head_length: f64) {
    self.fbb_.push_slot::<f64>(ArrowPrimitive::VT_HEAD_LENGTH, head_length, 0.0);
  }
  #[inline]
  pub fn add_head_diameter(&mut self, head_diameter: f64) {
    self.fbb_.push_slot::<f64>(ArrowPrimitive::VT_HEAD_DIAMETER, head_diameter, 0.0);
  }
  #[inline]
  pub fn add_color(&mut self, color: ::flatbuffers::WIPOffset<Color<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Color>>(ArrowPrimitive::VT_COLOR, color);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> ArrowPrimitiveBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    ArrowPrimitiveBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<ArrowPrimitive<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for ArrowPrimitive<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("ArrowPrimitive");
      ds.field("pose", &self.pose());
      ds.field("shaft_length", &self.shaft_length());
      ds.field("shaft_diameter", &self.shaft_diameter());
      ds.field("head_length", &self.head_length());
      ds.field("head_diameter", &self.head_diameter());
      ds.field("color", &self.color());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `ArrowPrimitive`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_arrow_primitive_unchecked`.
pub fn root_as_arrow_primitive(buf: &[u8]) -> Result<ArrowPrimitive<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<ArrowPrimitive>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `ArrowPrimitive` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_arrow_primitive_unchecked`.
pub fn size_prefixed_root_as_arrow_primitive(buf: &[u8]) -> Result<ArrowPrimitive<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<ArrowPrimitive>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `ArrowPrimitive` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_arrow_primitive_unchecked`.
pub fn root_as_arrow_primitive_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<ArrowPrimitive<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<ArrowPrimitive<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `ArrowPrimitive` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_arrow_primitive_unchecked`.
pub fn size_prefixed_root_as_arrow_primitive_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<ArrowPrimitive<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<ArrowPrimitive<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a ArrowPrimitive and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `ArrowPrimitive`.
pub unsafe fn root_as_arrow_primitive_unchecked(buf: &[u8]) -> ArrowPrimitive<'_> {
  unsafe { ::flatbuffers::root_unchecked::<ArrowPrimitive>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed ArrowPrimitive and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `ArrowPrimitive`.
pub unsafe fn size_prefixed_root_as_arrow_primitive_unchecked(buf: &[u8]) -> ArrowPrimitive<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<ArrowPrimitive>(buf) }
}
#[inline]
pub fn finish_arrow_primitive_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<ArrowPrimitive<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_arrow_primitive_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<ArrowPrimitive<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from ByteVector_generated.rs =====



pub enum ByteVectorOffset {}
#[derive(Copy, Clone, PartialEq)]

/// Used for nesting byte vectors
pub struct ByteVector<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for ByteVector<'a> {
  type Inner = ByteVector<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> ByteVector<'a> {
  pub const VT_DATA: ::flatbuffers::VOffsetT = 4;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    ByteVector { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args ByteVectorArgs<'args>
  ) -> ::flatbuffers::WIPOffset<ByteVector<'bldr>> {
    let mut builder = ByteVectorBuilder::new(_fbb);
    if let Some(x) = args.data { builder.add_data(x); }
    builder.finish()
  }


  #[inline]
  pub fn data(&self) -> Option<::flatbuffers::Vector<'a, u8>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, u8>>>(ByteVector::VT_DATA, None)}
  }
}

impl ::flatbuffers::Verifiable for ByteVector<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, u8>>>("data", Self::VT_DATA, false)?
     .finish();
    Ok(())
  }
}
pub struct ByteVectorArgs<'a> {
    pub data: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, u8>>>,
}
impl<'a> Default for ByteVectorArgs<'a> {
  #[inline]
  fn default() -> Self {
    ByteVectorArgs {
      data: None,
    }
  }
}

pub struct ByteVectorBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> ByteVectorBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_data(&mut self, data: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , u8>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(ByteVector::VT_DATA, data);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> ByteVectorBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    ByteVectorBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<ByteVector<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for ByteVector<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("ByteVector");
      ds.field("data", &self.data());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `ByteVector`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_byte_vector_unchecked`.
pub fn root_as_byte_vector(buf: &[u8]) -> Result<ByteVector<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<ByteVector>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `ByteVector` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_byte_vector_unchecked`.
pub fn size_prefixed_root_as_byte_vector(buf: &[u8]) -> Result<ByteVector<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<ByteVector>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `ByteVector` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_byte_vector_unchecked`.
pub fn root_as_byte_vector_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<ByteVector<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<ByteVector<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `ByteVector` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_byte_vector_unchecked`.
pub fn size_prefixed_root_as_byte_vector_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<ByteVector<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<ByteVector<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a ByteVector and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `ByteVector`.
pub unsafe fn root_as_byte_vector_unchecked(buf: &[u8]) -> ByteVector<'_> {
  unsafe { ::flatbuffers::root_unchecked::<ByteVector>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed ByteVector and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `ByteVector`.
pub unsafe fn size_prefixed_root_as_byte_vector_unchecked(buf: &[u8]) -> ByteVector<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<ByteVector>(buf) }
}
#[inline]
pub fn finish_byte_vector_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<ByteVector<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_byte_vector_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<ByteVector<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from CameraCalibration_generated.rs =====



pub enum CameraCalibrationOffset {}
#[derive(Copy, Clone, PartialEq)]

/// Camera calibration parameters
pub struct CameraCalibration<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for CameraCalibration<'a> {
  type Inner = CameraCalibration<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> CameraCalibration<'a> {
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
  pub const VT_FRAME_ID: ::flatbuffers::VOffsetT = 6;
  pub const VT_WIDTH: ::flatbuffers::VOffsetT = 8;
  pub const VT_HEIGHT: ::flatbuffers::VOffsetT = 10;
  pub const VT_DISTORTION_MODEL: ::flatbuffers::VOffsetT = 12;
  pub const VT_D: ::flatbuffers::VOffsetT = 14;
  pub const VT_K: ::flatbuffers::VOffsetT = 16;
  pub const VT_R: ::flatbuffers::VOffsetT = 18;
  pub const VT_P: ::flatbuffers::VOffsetT = 20;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    CameraCalibration { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args CameraCalibrationArgs<'args>
  ) -> ::flatbuffers::WIPOffset<CameraCalibration<'bldr>> {
    let mut builder = CameraCalibrationBuilder::new(_fbb);
    if let Some(x) = args.p { builder.add_p(x); }
    if let Some(x) = args.r { builder.add_r(x); }
    if let Some(x) = args.k { builder.add_k(x); }
    if let Some(x) = args.d { builder.add_d(x); }
    if let Some(x) = args.distortion_model { builder.add_distortion_model(x); }
    builder.add_height(args.height);
    builder.add_width(args.width);
    if let Some(x) = args.frame_id { builder.add_frame_id(x); }
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    builder.finish()
  }


  /// Timestamp of calibration data
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(CameraCalibration::VT_TIMESTAMP, None)}
  }
  /// Frame of reference for the camera. The origin of the frame is the optical center of the camera. +x points to the right in the image, +y points down, and +z points into the plane of the image.
  #[inline]
  pub fn frame_id(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(CameraCalibration::VT_FRAME_ID, None)}
  }
  /// Image width
  #[inline]
  pub fn width(&self) -> u32 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<u32>(CameraCalibration::VT_WIDTH, Some(0)).unwrap()}
  }
  /// Image height
  #[inline]
  pub fn height(&self) -> u32 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<u32>(CameraCalibration::VT_HEIGHT, Some(0)).unwrap()}
  }
  /// Name of distortion model
  /// 
  /// Supported parameters: `plumb_bob` (k1, k2, p1, p2, k3), `rational_polynomial` (k1, k2, p1, p2, k3, k4, k5, k6), and `kannala_brandt` (k1, k2, k3, k4), and `fisheye62` (k0, k1, k2, k3, p0, p1, crit_theta [optional]). `plumb_bob` and `rational_polynomial` models are based on the pinhole model [OpenCV's](https://docs.opencv.org/4.11.0/d9/d0c/group__calib3d.html) [pinhole camera model](https://en.wikipedia.org/wiki/Distortion_%28optics%29#Software_correction). The `kannala_brandt` model matches the [OpenvCV fisheye](https://docs.opencv.org/4.11.0/db/d58/group__calib3d__fisheye.html) model. The `fisheye62` model matches the [Project Aria's Fisheye62 Model](https://facebookresearch.github.io/projectaria_tools/docs/tech_insights/camera_intrinsic_models).
  #[inline]
  pub fn distortion_model(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(CameraCalibration::VT_DISTORTION_MODEL, None)}
  }
  /// Distortion parameters
  #[inline]
  pub fn d(&self) -> Option<::flatbuffers::Vector<'a, f64>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, f64>>>(CameraCalibration::VT_D, None)}
  }
  /// Intrinsic camera matrix (3x3 row-major matrix)
  /// 
  /// A 3x3 row-major matrix for the raw (distorted) image.
  /// 
  /// Projects 3D points in the camera coordinate frame to 2D pixel coordinates using the focal lengths (fx, fy) and principal point (cx, cy).
  /// 
  /// ```
  ///     [fx  0 cx]
  /// K = [ 0 fy cy]
  ///     [ 0  0  1]
  /// ```
  /// 
  /// **Uncalibrated cameras:** Following ROS conventions for [CameraInfo](https://docs.ros.org/en/noetic/api/sensor_msgs/html/msg/CameraInfo.html), Foxglove also treats K[0] == 0.0 as indicating an uncalibrated camera, and calibration data will be ignored.
  /// length 9
  #[inline]
  pub fn k(&self) -> Option<::flatbuffers::Vector<'a, f64>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, f64>>>(CameraCalibration::VT_K, None)}
  }
  /// Rectification matrix (stereo cameras only, 3x3 row-major matrix)
  /// 
  /// A rotation matrix aligning the camera coordinate system to the ideal stereo image plane so that epipolar lines in both stereo images are parallel.
  /// length 9
  #[inline]
  pub fn r(&self) -> Option<::flatbuffers::Vector<'a, f64>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, f64>>>(CameraCalibration::VT_R, None)}
  }
  /// Projection/camera matrix (3x4 row-major matrix)
  /// 
  /// ```
  ///     [fx'  0  cx' Tx]
  /// P = [ 0  fy' cy' Ty]
  ///     [ 0   0   1   0]
  /// ```
  /// 
  /// By convention, this matrix specifies the intrinsic (camera) matrix of the processed (rectified) image. That is, the left 3x3 portion is the normal camera intrinsic matrix for the rectified image.
  /// 
  /// It projects 3D points in the camera coordinate frame to 2D pixel coordinates using the focal lengths (fx', fy') and principal point (cx', cy') - these may differ from the values in K.
  /// 
  /// For monocular cameras, Tx = Ty = 0. Normally, monocular cameras will also have R = the identity and P[1:3,1:3] = K.
  /// 
  /// Foxglove currently does not support displaying stereo images, so Tx and Ty are ignored.
  /// 
  /// Given a 3D point [X Y Z]', the projection (x, y) of the point onto the rectified image is given by:
  /// 
  /// ```
  /// [u v w]' = P * [X Y Z 1]'
  ///        x = u / w
  ///        y = v / w
  /// ```
  /// 
  /// This holds for both images of a stereo pair.
  /// length 12
  #[inline]
  pub fn p(&self) -> Option<::flatbuffers::Vector<'a, f64>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, f64>>>(CameraCalibration::VT_P, None)}
  }
}

impl ::flatbuffers::Verifiable for CameraCalibration<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("frame_id", Self::VT_FRAME_ID, false)?
     .visit_field::<u32>("width", Self::VT_WIDTH, false)?
     .visit_field::<u32>("height", Self::VT_HEIGHT, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("distortion_model", Self::VT_DISTORTION_MODEL, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, f64>>>("d", Self::VT_D, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, f64>>>("k", Self::VT_K, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, f64>>>("r", Self::VT_R, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, f64>>>("p", Self::VT_P, false)?
     .finish();
    Ok(())
  }
}
pub struct CameraCalibrationArgs<'a> {
    pub timestamp: Option<&'a Time>,
    pub frame_id: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub width: u32,
    pub height: u32,
    pub distortion_model: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub d: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, f64>>>,
    pub k: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, f64>>>,
    pub r: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, f64>>>,
    pub p: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, f64>>>,
}
impl<'a> Default for CameraCalibrationArgs<'a> {
  #[inline]
  fn default() -> Self {
    CameraCalibrationArgs {
      timestamp: None,
      frame_id: None,
      width: 0,
      height: 0,
      distortion_model: None,
      d: None,
      k: None,
      r: None,
      p: None,
    }
  }
}

pub struct CameraCalibrationBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> CameraCalibrationBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(CameraCalibration::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn add_frame_id(&mut self, frame_id: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(CameraCalibration::VT_FRAME_ID, frame_id);
  }
  #[inline]
  pub fn add_width(&mut self, width: u32) {
    self.fbb_.push_slot::<u32>(CameraCalibration::VT_WIDTH, width, 0);
  }
  #[inline]
  pub fn add_height(&mut self, height: u32) {
    self.fbb_.push_slot::<u32>(CameraCalibration::VT_HEIGHT, height, 0);
  }
  #[inline]
  pub fn add_distortion_model(&mut self, distortion_model: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(CameraCalibration::VT_DISTORTION_MODEL, distortion_model);
  }
  #[inline]
  pub fn add_d(&mut self, d: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , f64>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(CameraCalibration::VT_D, d);
  }
  #[inline]
  pub fn add_k(&mut self, k: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , f64>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(CameraCalibration::VT_K, k);
  }
  #[inline]
  pub fn add_r(&mut self, r: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , f64>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(CameraCalibration::VT_R, r);
  }
  #[inline]
  pub fn add_p(&mut self, p: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , f64>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(CameraCalibration::VT_P, p);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> CameraCalibrationBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    CameraCalibrationBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<CameraCalibration<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for CameraCalibration<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("CameraCalibration");
      ds.field("timestamp", &self.timestamp());
      ds.field("frame_id", &self.frame_id());
      ds.field("width", &self.width());
      ds.field("height", &self.height());
      ds.field("distortion_model", &self.distortion_model());
      ds.field("d", &self.d());
      ds.field("k", &self.k());
      ds.field("r", &self.r());
      ds.field("p", &self.p());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `CameraCalibration`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_camera_calibration_unchecked`.
pub fn root_as_camera_calibration(buf: &[u8]) -> Result<CameraCalibration<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<CameraCalibration>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `CameraCalibration` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_camera_calibration_unchecked`.
pub fn size_prefixed_root_as_camera_calibration(buf: &[u8]) -> Result<CameraCalibration<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<CameraCalibration>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `CameraCalibration` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_camera_calibration_unchecked`.
pub fn root_as_camera_calibration_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<CameraCalibration<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<CameraCalibration<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `CameraCalibration` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_camera_calibration_unchecked`.
pub fn size_prefixed_root_as_camera_calibration_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<CameraCalibration<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<CameraCalibration<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a CameraCalibration and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `CameraCalibration`.
pub unsafe fn root_as_camera_calibration_unchecked(buf: &[u8]) -> CameraCalibration<'_> {
  unsafe { ::flatbuffers::root_unchecked::<CameraCalibration>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed CameraCalibration and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `CameraCalibration`.
pub unsafe fn size_prefixed_root_as_camera_calibration_unchecked(buf: &[u8]) -> CameraCalibration<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<CameraCalibration>(buf) }
}
#[inline]
pub fn finish_camera_calibration_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<CameraCalibration<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_camera_calibration_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<CameraCalibration<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from CircleAnnotation_generated.rs =====



pub enum CircleAnnotationOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A circle annotation on a 2D image
pub struct CircleAnnotation<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for CircleAnnotation<'a> {
  type Inner = CircleAnnotation<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> CircleAnnotation<'a> {
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
  pub const VT_POSITION: ::flatbuffers::VOffsetT = 6;
  pub const VT_DIAMETER: ::flatbuffers::VOffsetT = 8;
  pub const VT_THICKNESS: ::flatbuffers::VOffsetT = 10;
  pub const VT_FILL_COLOR: ::flatbuffers::VOffsetT = 12;
  pub const VT_OUTLINE_COLOR: ::flatbuffers::VOffsetT = 14;
  pub const VT_METADATA: ::flatbuffers::VOffsetT = 16;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    CircleAnnotation { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args CircleAnnotationArgs<'args>
  ) -> ::flatbuffers::WIPOffset<CircleAnnotation<'bldr>> {
    let mut builder = CircleAnnotationBuilder::new(_fbb);
    builder.add_thickness(args.thickness);
    builder.add_diameter(args.diameter);
    if let Some(x) = args.metadata { builder.add_metadata(x); }
    if let Some(x) = args.outline_color { builder.add_outline_color(x); }
    if let Some(x) = args.fill_color { builder.add_fill_color(x); }
    if let Some(x) = args.position { builder.add_position(x); }
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    builder.finish()
  }


  /// Timestamp of circle
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(CircleAnnotation::VT_TIMESTAMP, None)}
  }
  /// Center of the circle in 2D image coordinates (pixels).
  /// The coordinate uses the top-left corner of the top-left pixel of the image as the origin.
  #[inline]
  pub fn position(&self) -> Option<Point2<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Point2>>(CircleAnnotation::VT_POSITION, None)}
  }
  /// Circle diameter in pixels
  #[inline]
  pub fn diameter(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(CircleAnnotation::VT_DIAMETER, Some(0.0)).unwrap()}
  }
  /// Line thickness in pixels
  #[inline]
  pub fn thickness(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(CircleAnnotation::VT_THICKNESS, Some(0.0)).unwrap()}
  }
  /// Fill color
  #[inline]
  pub fn fill_color(&self) -> Option<Color<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Color>>(CircleAnnotation::VT_FILL_COLOR, None)}
  }
  /// Outline color
  #[inline]
  pub fn outline_color(&self) -> Option<Color<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Color>>(CircleAnnotation::VT_OUTLINE_COLOR, None)}
  }
  /// Additional user-provided metadata associated with this annotation. Keys must be unique.
  #[inline]
  pub fn metadata(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<KeyValuePair<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<KeyValuePair>>>>(CircleAnnotation::VT_METADATA, None)}
  }
}

impl ::flatbuffers::Verifiable for CircleAnnotation<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Point2>>("position", Self::VT_POSITION, false)?
     .visit_field::<f64>("diameter", Self::VT_DIAMETER, false)?
     .visit_field::<f64>("thickness", Self::VT_THICKNESS, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Color>>("fill_color", Self::VT_FILL_COLOR, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Color>>("outline_color", Self::VT_OUTLINE_COLOR, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<KeyValuePair>>>>("metadata", Self::VT_METADATA, false)?
     .finish();
    Ok(())
  }
}
pub struct CircleAnnotationArgs<'a> {
    pub timestamp: Option<&'a Time>,
    pub position: Option<::flatbuffers::WIPOffset<Point2<'a>>>,
    pub diameter: f64,
    pub thickness: f64,
    pub fill_color: Option<::flatbuffers::WIPOffset<Color<'a>>>,
    pub outline_color: Option<::flatbuffers::WIPOffset<Color<'a>>>,
    pub metadata: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<KeyValuePair<'a>>>>>,
}
impl<'a> Default for CircleAnnotationArgs<'a> {
  #[inline]
  fn default() -> Self {
    CircleAnnotationArgs {
      timestamp: None,
      position: None,
      diameter: 0.0,
      thickness: 0.0,
      fill_color: None,
      outline_color: None,
      metadata: None,
    }
  }
}

pub struct CircleAnnotationBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> CircleAnnotationBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(CircleAnnotation::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn add_position(&mut self, position: ::flatbuffers::WIPOffset<Point2<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Point2>>(CircleAnnotation::VT_POSITION, position);
  }
  #[inline]
  pub fn add_diameter(&mut self, diameter: f64) {
    self.fbb_.push_slot::<f64>(CircleAnnotation::VT_DIAMETER, diameter, 0.0);
  }
  #[inline]
  pub fn add_thickness(&mut self, thickness: f64) {
    self.fbb_.push_slot::<f64>(CircleAnnotation::VT_THICKNESS, thickness, 0.0);
  }
  #[inline]
  pub fn add_fill_color(&mut self, fill_color: ::flatbuffers::WIPOffset<Color<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Color>>(CircleAnnotation::VT_FILL_COLOR, fill_color);
  }
  #[inline]
  pub fn add_outline_color(&mut self, outline_color: ::flatbuffers::WIPOffset<Color<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Color>>(CircleAnnotation::VT_OUTLINE_COLOR, outline_color);
  }
  #[inline]
  pub fn add_metadata(&mut self, metadata: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<KeyValuePair<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(CircleAnnotation::VT_METADATA, metadata);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> CircleAnnotationBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    CircleAnnotationBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<CircleAnnotation<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for CircleAnnotation<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("CircleAnnotation");
      ds.field("timestamp", &self.timestamp());
      ds.field("position", &self.position());
      ds.field("diameter", &self.diameter());
      ds.field("thickness", &self.thickness());
      ds.field("fill_color", &self.fill_color());
      ds.field("outline_color", &self.outline_color());
      ds.field("metadata", &self.metadata());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `CircleAnnotation`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_circle_annotation_unchecked`.
pub fn root_as_circle_annotation(buf: &[u8]) -> Result<CircleAnnotation<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<CircleAnnotation>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `CircleAnnotation` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_circle_annotation_unchecked`.
pub fn size_prefixed_root_as_circle_annotation(buf: &[u8]) -> Result<CircleAnnotation<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<CircleAnnotation>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `CircleAnnotation` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_circle_annotation_unchecked`.
pub fn root_as_circle_annotation_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<CircleAnnotation<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<CircleAnnotation<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `CircleAnnotation` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_circle_annotation_unchecked`.
pub fn size_prefixed_root_as_circle_annotation_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<CircleAnnotation<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<CircleAnnotation<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a CircleAnnotation and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `CircleAnnotation`.
pub unsafe fn root_as_circle_annotation_unchecked(buf: &[u8]) -> CircleAnnotation<'_> {
  unsafe { ::flatbuffers::root_unchecked::<CircleAnnotation>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed CircleAnnotation and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `CircleAnnotation`.
pub unsafe fn size_prefixed_root_as_circle_annotation_unchecked(buf: &[u8]) -> CircleAnnotation<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<CircleAnnotation>(buf) }
}
#[inline]
pub fn finish_circle_annotation_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<CircleAnnotation<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_circle_annotation_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<CircleAnnotation<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from Color_generated.rs =====



pub enum ColorOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A color in RGBA format
pub struct Color<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for Color<'a> {
  type Inner = Color<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> Color<'a> {
  pub const VT_R: ::flatbuffers::VOffsetT = 4;
  pub const VT_G: ::flatbuffers::VOffsetT = 6;
  pub const VT_B: ::flatbuffers::VOffsetT = 8;
  pub const VT_A: ::flatbuffers::VOffsetT = 10;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    Color { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args ColorArgs
  ) -> ::flatbuffers::WIPOffset<Color<'bldr>> {
    let mut builder = ColorBuilder::new(_fbb);
    builder.add_a(args.a);
    builder.add_b(args.b);
    builder.add_g(args.g);
    builder.add_r(args.r);
    builder.finish()
  }


  /// Red value between 0 and 1
  #[inline]
  pub fn r(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(Color::VT_R, Some(1.0)).unwrap()}
  }
  /// Green value between 0 and 1
  #[inline]
  pub fn g(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(Color::VT_G, Some(1.0)).unwrap()}
  }
  /// Blue value between 0 and 1
  #[inline]
  pub fn b(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(Color::VT_B, Some(1.0)).unwrap()}
  }
  /// Alpha value between 0 and 1
  #[inline]
  pub fn a(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(Color::VT_A, Some(1.0)).unwrap()}
  }
}

impl ::flatbuffers::Verifiable for Color<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<f64>("r", Self::VT_R, false)?
     .visit_field::<f64>("g", Self::VT_G, false)?
     .visit_field::<f64>("b", Self::VT_B, false)?
     .visit_field::<f64>("a", Self::VT_A, false)?
     .finish();
    Ok(())
  }
}
pub struct ColorArgs {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}
impl<'a> Default for ColorArgs {
  #[inline]
  fn default() -> Self {
    ColorArgs {
      r: 1.0,
      g: 1.0,
      b: 1.0,
      a: 1.0,
    }
  }
}

pub struct ColorBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> ColorBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_r(&mut self, r: f64) {
    self.fbb_.push_slot::<f64>(Color::VT_R, r, 1.0);
  }
  #[inline]
  pub fn add_g(&mut self, g: f64) {
    self.fbb_.push_slot::<f64>(Color::VT_G, g, 1.0);
  }
  #[inline]
  pub fn add_b(&mut self, b: f64) {
    self.fbb_.push_slot::<f64>(Color::VT_B, b, 1.0);
  }
  #[inline]
  pub fn add_a(&mut self, a: f64) {
    self.fbb_.push_slot::<f64>(Color::VT_A, a, 1.0);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> ColorBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    ColorBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<Color<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for Color<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("Color");
      ds.field("r", &self.r());
      ds.field("g", &self.g());
      ds.field("b", &self.b());
      ds.field("a", &self.a());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `Color`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_color_unchecked`.
pub fn root_as_color(buf: &[u8]) -> Result<Color<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<Color>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `Color` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_color_unchecked`.
pub fn size_prefixed_root_as_color(buf: &[u8]) -> Result<Color<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<Color>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `Color` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_color_unchecked`.
pub fn root_as_color_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<Color<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<Color<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `Color` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_color_unchecked`.
pub fn size_prefixed_root_as_color_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<Color<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<Color<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a Color and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `Color`.
pub unsafe fn root_as_color_unchecked(buf: &[u8]) -> Color<'_> {
  unsafe { ::flatbuffers::root_unchecked::<Color>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed Color and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `Color`.
pub unsafe fn size_prefixed_root_as_color_unchecked(buf: &[u8]) -> Color<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<Color>(buf) }
}
#[inline]
pub fn finish_color_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<Color<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_color_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<Color<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from CompressedImage_generated.rs =====



pub enum CompressedImageOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A compressed image
pub struct CompressedImage<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for CompressedImage<'a> {
  type Inner = CompressedImage<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> CompressedImage<'a> {
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
  pub const VT_FRAME_ID: ::flatbuffers::VOffsetT = 6;
  pub const VT_DATA: ::flatbuffers::VOffsetT = 8;
  pub const VT_FORMAT: ::flatbuffers::VOffsetT = 10;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    CompressedImage { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args CompressedImageArgs<'args>
  ) -> ::flatbuffers::WIPOffset<CompressedImage<'bldr>> {
    let mut builder = CompressedImageBuilder::new(_fbb);
    if let Some(x) = args.format { builder.add_format(x); }
    if let Some(x) = args.data { builder.add_data(x); }
    if let Some(x) = args.frame_id { builder.add_frame_id(x); }
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    builder.finish()
  }


  /// Timestamp of image
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(CompressedImage::VT_TIMESTAMP, None)}
  }
  /// Frame of reference for the image. The origin of the frame is the optical center of the camera. +x points to the right in the image, +y points down, and +z points into the plane of the image.
  #[inline]
  pub fn frame_id(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(CompressedImage::VT_FRAME_ID, None)}
  }
  /// Compressed image data
  #[inline]
  pub fn data(&self) -> Option<::flatbuffers::Vector<'a, u8>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, u8>>>(CompressedImage::VT_DATA, None)}
  }
  /// Image format
  /// 
  /// Supported values: `jpeg`, `png`, `webp`, `avif`
  #[inline]
  pub fn format(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(CompressedImage::VT_FORMAT, None)}
  }
}

impl ::flatbuffers::Verifiable for CompressedImage<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("frame_id", Self::VT_FRAME_ID, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, u8>>>("data", Self::VT_DATA, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("format", Self::VT_FORMAT, false)?
     .finish();
    Ok(())
  }
}
pub struct CompressedImageArgs<'a> {
    pub timestamp: Option<&'a Time>,
    pub frame_id: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub data: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, u8>>>,
    pub format: Option<::flatbuffers::WIPOffset<&'a str>>,
}
impl<'a> Default for CompressedImageArgs<'a> {
  #[inline]
  fn default() -> Self {
    CompressedImageArgs {
      timestamp: None,
      frame_id: None,
      data: None,
      format: None,
    }
  }
}

pub struct CompressedImageBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> CompressedImageBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(CompressedImage::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn add_frame_id(&mut self, frame_id: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(CompressedImage::VT_FRAME_ID, frame_id);
  }
  #[inline]
  pub fn add_data(&mut self, data: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , u8>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(CompressedImage::VT_DATA, data);
  }
  #[inline]
  pub fn add_format(&mut self, format: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(CompressedImage::VT_FORMAT, format);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> CompressedImageBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    CompressedImageBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<CompressedImage<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for CompressedImage<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("CompressedImage");
      ds.field("timestamp", &self.timestamp());
      ds.field("frame_id", &self.frame_id());
      ds.field("data", &self.data());
      ds.field("format", &self.format());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `CompressedImage`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_compressed_image_unchecked`.
pub fn root_as_compressed_image(buf: &[u8]) -> Result<CompressedImage<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<CompressedImage>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `CompressedImage` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_compressed_image_unchecked`.
pub fn size_prefixed_root_as_compressed_image(buf: &[u8]) -> Result<CompressedImage<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<CompressedImage>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `CompressedImage` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_compressed_image_unchecked`.
pub fn root_as_compressed_image_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<CompressedImage<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<CompressedImage<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `CompressedImage` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_compressed_image_unchecked`.
pub fn size_prefixed_root_as_compressed_image_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<CompressedImage<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<CompressedImage<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a CompressedImage and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `CompressedImage`.
pub unsafe fn root_as_compressed_image_unchecked(buf: &[u8]) -> CompressedImage<'_> {
  unsafe { ::flatbuffers::root_unchecked::<CompressedImage>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed CompressedImage and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `CompressedImage`.
pub unsafe fn size_prefixed_root_as_compressed_image_unchecked(buf: &[u8]) -> CompressedImage<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<CompressedImage>(buf) }
}
#[inline]
pub fn finish_compressed_image_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<CompressedImage<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_compressed_image_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<CompressedImage<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from CompressedPointCloud_generated.rs =====



pub enum CompressedPointCloudOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A compressed point cloud. A decoder for `format` must decompress `data`, using metadata stored in the compressed payload to recover point positions and any additional per-point attributes. The decoded point cloud must include at least 2 coordinate fields from `x`, `y`, and `z`; `red`, `green`, `blue`, and `alpha` are optional for customizing each point's color.
pub struct CompressedPointCloud<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for CompressedPointCloud<'a> {
  type Inner = CompressedPointCloud<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> CompressedPointCloud<'a> {
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
  pub const VT_FRAME_ID: ::flatbuffers::VOffsetT = 6;
  pub const VT_POSE: ::flatbuffers::VOffsetT = 8;
  pub const VT_DATA: ::flatbuffers::VOffsetT = 10;
  pub const VT_FORMAT: ::flatbuffers::VOffsetT = 12;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    CompressedPointCloud { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args CompressedPointCloudArgs<'args>
  ) -> ::flatbuffers::WIPOffset<CompressedPointCloud<'bldr>> {
    let mut builder = CompressedPointCloudBuilder::new(_fbb);
    if let Some(x) = args.format { builder.add_format(x); }
    if let Some(x) = args.data { builder.add_data(x); }
    if let Some(x) = args.pose { builder.add_pose(x); }
    if let Some(x) = args.frame_id { builder.add_frame_id(x); }
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    builder.finish()
  }


  /// Timestamp of point cloud
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(CompressedPointCloud::VT_TIMESTAMP, None)}
  }
  /// Frame of reference
  #[inline]
  pub fn frame_id(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(CompressedPointCloud::VT_FRAME_ID, None)}
  }
  /// The origin of the point cloud relative to the frame of reference
  #[inline]
  pub fn pose(&self) -> Option<Pose<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Pose>>(CompressedPointCloud::VT_POSE, None)}
  }
  /// Compressed point cloud data for exactly one point cloud, including any format-specific metadata needed to describe the decoded point attributes.
  #[inline]
  pub fn data(&self) -> Option<::flatbuffers::Vector<'a, u8>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, u8>>>(CompressedPointCloud::VT_DATA, None)}
  }
  /// Point cloud compression format.
  /// 
  /// Supported values: `draco` ([Google Draco](https://google.github.io/draco/)).
  #[inline]
  pub fn format(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(CompressedPointCloud::VT_FORMAT, None)}
  }
}

impl ::flatbuffers::Verifiable for CompressedPointCloud<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("frame_id", Self::VT_FRAME_ID, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Pose>>("pose", Self::VT_POSE, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, u8>>>("data", Self::VT_DATA, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("format", Self::VT_FORMAT, false)?
     .finish();
    Ok(())
  }
}
pub struct CompressedPointCloudArgs<'a> {
    pub timestamp: Option<&'a Time>,
    pub frame_id: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub pose: Option<::flatbuffers::WIPOffset<Pose<'a>>>,
    pub data: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, u8>>>,
    pub format: Option<::flatbuffers::WIPOffset<&'a str>>,
}
impl<'a> Default for CompressedPointCloudArgs<'a> {
  #[inline]
  fn default() -> Self {
    CompressedPointCloudArgs {
      timestamp: None,
      frame_id: None,
      pose: None,
      data: None,
      format: None,
    }
  }
}

pub struct CompressedPointCloudBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> CompressedPointCloudBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(CompressedPointCloud::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn add_frame_id(&mut self, frame_id: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(CompressedPointCloud::VT_FRAME_ID, frame_id);
  }
  #[inline]
  pub fn add_pose(&mut self, pose: ::flatbuffers::WIPOffset<Pose<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Pose>>(CompressedPointCloud::VT_POSE, pose);
  }
  #[inline]
  pub fn add_data(&mut self, data: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , u8>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(CompressedPointCloud::VT_DATA, data);
  }
  #[inline]
  pub fn add_format(&mut self, format: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(CompressedPointCloud::VT_FORMAT, format);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> CompressedPointCloudBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    CompressedPointCloudBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<CompressedPointCloud<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for CompressedPointCloud<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("CompressedPointCloud");
      ds.field("timestamp", &self.timestamp());
      ds.field("frame_id", &self.frame_id());
      ds.field("pose", &self.pose());
      ds.field("data", &self.data());
      ds.field("format", &self.format());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `CompressedPointCloud`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_compressed_point_cloud_unchecked`.
pub fn root_as_compressed_point_cloud(buf: &[u8]) -> Result<CompressedPointCloud<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<CompressedPointCloud>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `CompressedPointCloud` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_compressed_point_cloud_unchecked`.
pub fn size_prefixed_root_as_compressed_point_cloud(buf: &[u8]) -> Result<CompressedPointCloud<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<CompressedPointCloud>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `CompressedPointCloud` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_compressed_point_cloud_unchecked`.
pub fn root_as_compressed_point_cloud_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<CompressedPointCloud<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<CompressedPointCloud<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `CompressedPointCloud` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_compressed_point_cloud_unchecked`.
pub fn size_prefixed_root_as_compressed_point_cloud_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<CompressedPointCloud<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<CompressedPointCloud<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a CompressedPointCloud and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `CompressedPointCloud`.
pub unsafe fn root_as_compressed_point_cloud_unchecked(buf: &[u8]) -> CompressedPointCloud<'_> {
  unsafe { ::flatbuffers::root_unchecked::<CompressedPointCloud>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed CompressedPointCloud and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `CompressedPointCloud`.
pub unsafe fn size_prefixed_root_as_compressed_point_cloud_unchecked(buf: &[u8]) -> CompressedPointCloud<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<CompressedPointCloud>(buf) }
}
#[inline]
pub fn finish_compressed_point_cloud_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<CompressedPointCloud<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_compressed_point_cloud_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<CompressedPointCloud<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from CompressedVideo_generated.rs =====



pub enum CompressedVideoOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A single frame of a compressed video bitstream
pub struct CompressedVideo<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for CompressedVideo<'a> {
  type Inner = CompressedVideo<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> CompressedVideo<'a> {
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
  pub const VT_FRAME_ID: ::flatbuffers::VOffsetT = 6;
  pub const VT_DATA: ::flatbuffers::VOffsetT = 8;
  pub const VT_FORMAT: ::flatbuffers::VOffsetT = 10;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    CompressedVideo { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args CompressedVideoArgs<'args>
  ) -> ::flatbuffers::WIPOffset<CompressedVideo<'bldr>> {
    let mut builder = CompressedVideoBuilder::new(_fbb);
    if let Some(x) = args.format { builder.add_format(x); }
    if let Some(x) = args.data { builder.add_data(x); }
    if let Some(x) = args.frame_id { builder.add_frame_id(x); }
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    builder.finish()
  }


  /// Timestamp of video frame
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(CompressedVideo::VT_TIMESTAMP, None)}
  }
  /// Frame of reference for the video.
  /// 
  /// The origin of the frame is the optical center of the camera. +x points to the right in the video, +y points down, and +z points into the plane of the video.
  #[inline]
  pub fn frame_id(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(CompressedVideo::VT_FRAME_ID, None)}
  }
  /// Compressed video frame data.
  /// 
  /// For packet-based video codecs this data must begin and end on packet boundaries (no partial packets), and must contain enough video packets to decode exactly one image (either a keyframe or delta frame). Note: Foxglove does not support video streams that include B frames because they require lookahead.
  /// 
  /// Specifically, the requirements for different `format` values are:
  /// 
  /// - `h264`
  ///   - Use Annex B formatted data
  ///   - Each CompressedVideo message should contain enough NAL units to decode exactly one video frame
  ///   - Each message containing a key frame (IDR) must also include a SPS NAL unit
  /// 
  /// - `h265` (HEVC)
  ///   - Use Annex B formatted data
  ///   - Each CompressedVideo message should contain enough NAL units to decode exactly one video frame
  ///   - Each message containing a key frame (IRAP) must also include relevant VPS/SPS/PPS NAL units
  /// 
  /// - `vp9`
  ///   - Each CompressedVideo message should contain exactly one video frame
  /// 
  /// - `av1`
  ///   - Use the "Low overhead bitstream format" (section 5.2)
  ///   - Each CompressedVideo message should contain enough OBUs to decode exactly one video frame
  ///   - Each message containing a key frame must also include a Sequence Header OBU
  #[inline]
  pub fn data(&self) -> Option<::flatbuffers::Vector<'a, u8>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, u8>>>(CompressedVideo::VT_DATA, None)}
  }
  /// Video format.
  /// 
  /// Supported values: `h264`, `h265`, `vp9`, `av1`.
  /// 
  /// Note: compressed video support is subject to hardware limitations and patent licensing, so not all encodings may be supported on all platforms. See more about [H.265 support](https://caniuse.com/hevc), [VP9 support](https://caniuse.com/webm), and [AV1 support](https://caniuse.com/av1).
  #[inline]
  pub fn format(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(CompressedVideo::VT_FORMAT, None)}
  }
}

impl ::flatbuffers::Verifiable for CompressedVideo<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("frame_id", Self::VT_FRAME_ID, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, u8>>>("data", Self::VT_DATA, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("format", Self::VT_FORMAT, false)?
     .finish();
    Ok(())
  }
}
pub struct CompressedVideoArgs<'a> {
    pub timestamp: Option<&'a Time>,
    pub frame_id: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub data: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, u8>>>,
    pub format: Option<::flatbuffers::WIPOffset<&'a str>>,
}
impl<'a> Default for CompressedVideoArgs<'a> {
  #[inline]
  fn default() -> Self {
    CompressedVideoArgs {
      timestamp: None,
      frame_id: None,
      data: None,
      format: None,
    }
  }
}

pub struct CompressedVideoBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> CompressedVideoBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(CompressedVideo::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn add_frame_id(&mut self, frame_id: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(CompressedVideo::VT_FRAME_ID, frame_id);
  }
  #[inline]
  pub fn add_data(&mut self, data: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , u8>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(CompressedVideo::VT_DATA, data);
  }
  #[inline]
  pub fn add_format(&mut self, format: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(CompressedVideo::VT_FORMAT, format);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> CompressedVideoBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    CompressedVideoBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<CompressedVideo<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for CompressedVideo<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("CompressedVideo");
      ds.field("timestamp", &self.timestamp());
      ds.field("frame_id", &self.frame_id());
      ds.field("data", &self.data());
      ds.field("format", &self.format());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `CompressedVideo`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_compressed_video_unchecked`.
pub fn root_as_compressed_video(buf: &[u8]) -> Result<CompressedVideo<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<CompressedVideo>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `CompressedVideo` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_compressed_video_unchecked`.
pub fn size_prefixed_root_as_compressed_video(buf: &[u8]) -> Result<CompressedVideo<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<CompressedVideo>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `CompressedVideo` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_compressed_video_unchecked`.
pub fn root_as_compressed_video_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<CompressedVideo<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<CompressedVideo<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `CompressedVideo` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_compressed_video_unchecked`.
pub fn size_prefixed_root_as_compressed_video_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<CompressedVideo<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<CompressedVideo<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a CompressedVideo and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `CompressedVideo`.
pub unsafe fn root_as_compressed_video_unchecked(buf: &[u8]) -> CompressedVideo<'_> {
  unsafe { ::flatbuffers::root_unchecked::<CompressedVideo>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed CompressedVideo and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `CompressedVideo`.
pub unsafe fn size_prefixed_root_as_compressed_video_unchecked(buf: &[u8]) -> CompressedVideo<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<CompressedVideo>(buf) }
}
#[inline]
pub fn finish_compressed_video_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<CompressedVideo<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_compressed_video_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<CompressedVideo<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from CubePrimitive_generated.rs =====



pub enum CubePrimitiveOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A primitive representing a cube or rectangular prism
pub struct CubePrimitive<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for CubePrimitive<'a> {
  type Inner = CubePrimitive<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> CubePrimitive<'a> {
  pub const VT_POSE: ::flatbuffers::VOffsetT = 4;
  pub const VT_SIZE: ::flatbuffers::VOffsetT = 6;
  pub const VT_COLOR: ::flatbuffers::VOffsetT = 8;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    CubePrimitive { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args CubePrimitiveArgs<'args>
  ) -> ::flatbuffers::WIPOffset<CubePrimitive<'bldr>> {
    let mut builder = CubePrimitiveBuilder::new(_fbb);
    if let Some(x) = args.color { builder.add_color(x); }
    if let Some(x) = args.size { builder.add_size(x); }
    if let Some(x) = args.pose { builder.add_pose(x); }
    builder.finish()
  }


  /// Position of the center of the cube and orientation of the cube
  #[inline]
  pub fn pose(&self) -> Option<Pose<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Pose>>(CubePrimitive::VT_POSE, None)}
  }
  /// Size of the cube along each axis
  #[inline]
  pub fn size(&self) -> Option<Vector3<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Vector3>>(CubePrimitive::VT_SIZE, None)}
  }
  /// Color of the cube
  #[inline]
  pub fn color(&self) -> Option<Color<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Color>>(CubePrimitive::VT_COLOR, None)}
  }
}

impl ::flatbuffers::Verifiable for CubePrimitive<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Pose>>("pose", Self::VT_POSE, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Vector3>>("size", Self::VT_SIZE, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Color>>("color", Self::VT_COLOR, false)?
     .finish();
    Ok(())
  }
}
pub struct CubePrimitiveArgs<'a> {
    pub pose: Option<::flatbuffers::WIPOffset<Pose<'a>>>,
    pub size: Option<::flatbuffers::WIPOffset<Vector3<'a>>>,
    pub color: Option<::flatbuffers::WIPOffset<Color<'a>>>,
}
impl<'a> Default for CubePrimitiveArgs<'a> {
  #[inline]
  fn default() -> Self {
    CubePrimitiveArgs {
      pose: None,
      size: None,
      color: None,
    }
  }
}

pub struct CubePrimitiveBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> CubePrimitiveBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_pose(&mut self, pose: ::flatbuffers::WIPOffset<Pose<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Pose>>(CubePrimitive::VT_POSE, pose);
  }
  #[inline]
  pub fn add_size(&mut self, size: ::flatbuffers::WIPOffset<Vector3<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Vector3>>(CubePrimitive::VT_SIZE, size);
  }
  #[inline]
  pub fn add_color(&mut self, color: ::flatbuffers::WIPOffset<Color<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Color>>(CubePrimitive::VT_COLOR, color);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> CubePrimitiveBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    CubePrimitiveBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<CubePrimitive<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for CubePrimitive<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("CubePrimitive");
      ds.field("pose", &self.pose());
      ds.field("size", &self.size());
      ds.field("color", &self.color());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `CubePrimitive`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_cube_primitive_unchecked`.
pub fn root_as_cube_primitive(buf: &[u8]) -> Result<CubePrimitive<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<CubePrimitive>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `CubePrimitive` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_cube_primitive_unchecked`.
pub fn size_prefixed_root_as_cube_primitive(buf: &[u8]) -> Result<CubePrimitive<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<CubePrimitive>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `CubePrimitive` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_cube_primitive_unchecked`.
pub fn root_as_cube_primitive_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<CubePrimitive<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<CubePrimitive<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `CubePrimitive` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_cube_primitive_unchecked`.
pub fn size_prefixed_root_as_cube_primitive_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<CubePrimitive<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<CubePrimitive<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a CubePrimitive and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `CubePrimitive`.
pub unsafe fn root_as_cube_primitive_unchecked(buf: &[u8]) -> CubePrimitive<'_> {
  unsafe { ::flatbuffers::root_unchecked::<CubePrimitive>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed CubePrimitive and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `CubePrimitive`.
pub unsafe fn size_prefixed_root_as_cube_primitive_unchecked(buf: &[u8]) -> CubePrimitive<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<CubePrimitive>(buf) }
}
#[inline]
pub fn finish_cube_primitive_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<CubePrimitive<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_cube_primitive_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<CubePrimitive<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from CylinderPrimitive_generated.rs =====



pub enum CylinderPrimitiveOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A primitive representing a cylinder, elliptic cylinder, or truncated cone
pub struct CylinderPrimitive<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for CylinderPrimitive<'a> {
  type Inner = CylinderPrimitive<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> CylinderPrimitive<'a> {
  pub const VT_POSE: ::flatbuffers::VOffsetT = 4;
  pub const VT_SIZE: ::flatbuffers::VOffsetT = 6;
  pub const VT_BOTTOM_SCALE: ::flatbuffers::VOffsetT = 8;
  pub const VT_TOP_SCALE: ::flatbuffers::VOffsetT = 10;
  pub const VT_COLOR: ::flatbuffers::VOffsetT = 12;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    CylinderPrimitive { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args CylinderPrimitiveArgs<'args>
  ) -> ::flatbuffers::WIPOffset<CylinderPrimitive<'bldr>> {
    let mut builder = CylinderPrimitiveBuilder::new(_fbb);
    builder.add_top_scale(args.top_scale);
    builder.add_bottom_scale(args.bottom_scale);
    if let Some(x) = args.color { builder.add_color(x); }
    if let Some(x) = args.size { builder.add_size(x); }
    if let Some(x) = args.pose { builder.add_pose(x); }
    builder.finish()
  }


  /// Position of the center of the cylinder and orientation of the cylinder. The flat face(s) are perpendicular to the z-axis.
  #[inline]
  pub fn pose(&self) -> Option<Pose<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Pose>>(CylinderPrimitive::VT_POSE, None)}
  }
  /// Size of the cylinder's bounding box
  #[inline]
  pub fn size(&self) -> Option<Vector3<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Vector3>>(CylinderPrimitive::VT_SIZE, None)}
  }
  /// 0-1, ratio of the diameter of the cylinder's bottom face (min z) to the bottom of the bounding box
  #[inline]
  pub fn bottom_scale(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(CylinderPrimitive::VT_BOTTOM_SCALE, Some(0.0)).unwrap()}
  }
  /// 0-1, ratio of the diameter of the cylinder's top face (max z) to the top of the bounding box
  #[inline]
  pub fn top_scale(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(CylinderPrimitive::VT_TOP_SCALE, Some(0.0)).unwrap()}
  }
  /// Color of the cylinder
  #[inline]
  pub fn color(&self) -> Option<Color<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Color>>(CylinderPrimitive::VT_COLOR, None)}
  }
}

impl ::flatbuffers::Verifiable for CylinderPrimitive<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Pose>>("pose", Self::VT_POSE, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Vector3>>("size", Self::VT_SIZE, false)?
     .visit_field::<f64>("bottom_scale", Self::VT_BOTTOM_SCALE, false)?
     .visit_field::<f64>("top_scale", Self::VT_TOP_SCALE, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Color>>("color", Self::VT_COLOR, false)?
     .finish();
    Ok(())
  }
}
pub struct CylinderPrimitiveArgs<'a> {
    pub pose: Option<::flatbuffers::WIPOffset<Pose<'a>>>,
    pub size: Option<::flatbuffers::WIPOffset<Vector3<'a>>>,
    pub bottom_scale: f64,
    pub top_scale: f64,
    pub color: Option<::flatbuffers::WIPOffset<Color<'a>>>,
}
impl<'a> Default for CylinderPrimitiveArgs<'a> {
  #[inline]
  fn default() -> Self {
    CylinderPrimitiveArgs {
      pose: None,
      size: None,
      bottom_scale: 0.0,
      top_scale: 0.0,
      color: None,
    }
  }
}

pub struct CylinderPrimitiveBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> CylinderPrimitiveBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_pose(&mut self, pose: ::flatbuffers::WIPOffset<Pose<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Pose>>(CylinderPrimitive::VT_POSE, pose);
  }
  #[inline]
  pub fn add_size(&mut self, size: ::flatbuffers::WIPOffset<Vector3<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Vector3>>(CylinderPrimitive::VT_SIZE, size);
  }
  #[inline]
  pub fn add_bottom_scale(&mut self, bottom_scale: f64) {
    self.fbb_.push_slot::<f64>(CylinderPrimitive::VT_BOTTOM_SCALE, bottom_scale, 0.0);
  }
  #[inline]
  pub fn add_top_scale(&mut self, top_scale: f64) {
    self.fbb_.push_slot::<f64>(CylinderPrimitive::VT_TOP_SCALE, top_scale, 0.0);
  }
  #[inline]
  pub fn add_color(&mut self, color: ::flatbuffers::WIPOffset<Color<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Color>>(CylinderPrimitive::VT_COLOR, color);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> CylinderPrimitiveBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    CylinderPrimitiveBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<CylinderPrimitive<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for CylinderPrimitive<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("CylinderPrimitive");
      ds.field("pose", &self.pose());
      ds.field("size", &self.size());
      ds.field("bottom_scale", &self.bottom_scale());
      ds.field("top_scale", &self.top_scale());
      ds.field("color", &self.color());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `CylinderPrimitive`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_cylinder_primitive_unchecked`.
pub fn root_as_cylinder_primitive(buf: &[u8]) -> Result<CylinderPrimitive<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<CylinderPrimitive>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `CylinderPrimitive` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_cylinder_primitive_unchecked`.
pub fn size_prefixed_root_as_cylinder_primitive(buf: &[u8]) -> Result<CylinderPrimitive<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<CylinderPrimitive>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `CylinderPrimitive` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_cylinder_primitive_unchecked`.
pub fn root_as_cylinder_primitive_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<CylinderPrimitive<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<CylinderPrimitive<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `CylinderPrimitive` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_cylinder_primitive_unchecked`.
pub fn size_prefixed_root_as_cylinder_primitive_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<CylinderPrimitive<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<CylinderPrimitive<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a CylinderPrimitive and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `CylinderPrimitive`.
pub unsafe fn root_as_cylinder_primitive_unchecked(buf: &[u8]) -> CylinderPrimitive<'_> {
  unsafe { ::flatbuffers::root_unchecked::<CylinderPrimitive>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed CylinderPrimitive and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `CylinderPrimitive`.
pub unsafe fn size_prefixed_root_as_cylinder_primitive_unchecked(buf: &[u8]) -> CylinderPrimitive<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<CylinderPrimitive>(buf) }
}
#[inline]
pub fn finish_cylinder_primitive_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<CylinderPrimitive<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_cylinder_primitive_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<CylinderPrimitive<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from Duration_generated.rs =====



// struct Duration, aligned to 4
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq)]
pub struct Duration(pub [u8; 8]);
impl Default for Duration { 
  fn default() -> Self { 
    Self([0; 8])
  }
}
impl ::core::fmt::Debug for Duration {
  fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
    f.debug_struct("Duration")
      .field("sec", &self.sec())
      .field("nsec", &self.nsec())
      .finish()
  }
}

impl ::flatbuffers::SimpleToVerifyInSlice for Duration {}
impl<'a> ::flatbuffers::Follow<'a> for Duration {
  type Inner = &'a Duration;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    unsafe { <&'a Duration>::follow(buf, loc) }
  }
}
impl<'a> ::flatbuffers::Follow<'a> for &'a Duration {
  type Inner = &'a Duration;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    unsafe { ::flatbuffers::follow_cast_ref::<Duration>(buf, loc) }
  }
}
impl<'b> ::flatbuffers::Push for Duration {
    type Output = Duration;
    #[inline]
    unsafe fn push(&self, dst: &mut [u8], _written_len: usize) {
        let src = unsafe { ::core::slice::from_raw_parts(self as *const Duration as *const u8, <Self as ::flatbuffers::Push>::size()) };
        dst.copy_from_slice(src);
    }
    #[inline]
    fn alignment() -> ::flatbuffers::PushAlignment {
        ::flatbuffers::PushAlignment::new(4)
    }
}

impl<'a> ::flatbuffers::Verifiable for Duration {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.in_buffer::<Self>(pos)
  }
}

impl<'a> Duration {
  #[allow(clippy::too_many_arguments)]
  pub fn new(
    sec: i32,
    nsec: i32,
  ) -> Self {
    let mut s = Self([0; 8]);
    s.set_sec(sec);
    s.set_nsec(nsec);
    s
  }

  /// Signed seconds of the span of time.
  pub fn sec(&self) -> i32 {
    let mut mem = ::core::mem::MaybeUninit::<<i32 as ::flatbuffers::EndianScalar>::Scalar>::uninit();
    // Safety:
    // Created from a valid Table for this object
    // Which contains a valid value in this slot
    ::flatbuffers::EndianScalar::from_little_endian(unsafe {
      ::core::ptr::copy_nonoverlapping(
        self.0[0..].as_ptr(),
        mem.as_mut_ptr() as *mut u8,
        ::core::mem::size_of::<<i32 as ::flatbuffers::EndianScalar>::Scalar>(),
      );
      mem.assume_init()
    })
  }

  pub fn set_sec(&mut self, x: i32) {
    let x_le = ::flatbuffers::EndianScalar::to_little_endian(x);
    // Safety:
    // Created from a valid Table for this object
    // Which contains a valid value in this slot
    unsafe {
      ::core::ptr::copy_nonoverlapping(
        &x_le as *const _ as *const u8,
        self.0[0..].as_mut_ptr(),
        ::core::mem::size_of::<<i32 as ::flatbuffers::EndianScalar>::Scalar>(),
      );
    }
  }

  /// if sec === 0 : -999,999,999 <= nsec <= +999,999,999
  /// otherwise sign of sec must match sign of nsec or be 0 and abs(nsec) <= 999,999,999
  pub fn nsec(&self) -> i32 {
    let mut mem = ::core::mem::MaybeUninit::<<i32 as ::flatbuffers::EndianScalar>::Scalar>::uninit();
    // Safety:
    // Created from a valid Table for this object
    // Which contains a valid value in this slot
    ::flatbuffers::EndianScalar::from_little_endian(unsafe {
      ::core::ptr::copy_nonoverlapping(
        self.0[4..].as_ptr(),
        mem.as_mut_ptr() as *mut u8,
        ::core::mem::size_of::<<i32 as ::flatbuffers::EndianScalar>::Scalar>(),
      );
      mem.assume_init()
    })
  }

  pub fn set_nsec(&mut self, x: i32) {
    let x_le = ::flatbuffers::EndianScalar::to_little_endian(x);
    // Safety:
    // Created from a valid Table for this object
    // Which contains a valid value in this slot
    unsafe {
      ::core::ptr::copy_nonoverlapping(
        &x_le as *const _ as *const u8,
        self.0[4..].as_mut_ptr(),
        ::core::mem::size_of::<<i32 as ::flatbuffers::EndianScalar>::Scalar>(),
      );
    }
  }

}


// ===== from FrameTransform_generated.rs =====



pub enum FrameTransformOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A transform between two reference frames in 3D space. The transform defines the position and orientation of a child frame within a parent frame. Translation moves the origin of the child frame relative to the parent origin. The rotation changes the orientation of the child frame around its origin.
/// 
/// Examples:
/// 
/// - With translation (x=1, y=0, z=0) and identity rotation (x=0, y=0, z=0, w=1), a point at (x=0, y=0, z=0) in the child frame maps to (x=1, y=0, z=0) in the parent frame.
/// 
/// - With translation (x=1, y=2, z=0) and a 90-degree rotation around the z-axis (x=0, y=0, z=0.707, w=0.707), a point at (x=1, y=0, z=0) in the child frame maps to (x=-1, y=3, z=0) in the parent frame.
pub struct FrameTransform<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for FrameTransform<'a> {
  type Inner = FrameTransform<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> FrameTransform<'a> {
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
  pub const VT_PARENT_FRAME_ID: ::flatbuffers::VOffsetT = 6;
  pub const VT_CHILD_FRAME_ID: ::flatbuffers::VOffsetT = 8;
  pub const VT_TRANSLATION: ::flatbuffers::VOffsetT = 10;
  pub const VT_ROTATION: ::flatbuffers::VOffsetT = 12;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    FrameTransform { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args FrameTransformArgs<'args>
  ) -> ::flatbuffers::WIPOffset<FrameTransform<'bldr>> {
    let mut builder = FrameTransformBuilder::new(_fbb);
    if let Some(x) = args.rotation { builder.add_rotation(x); }
    if let Some(x) = args.translation { builder.add_translation(x); }
    if let Some(x) = args.child_frame_id { builder.add_child_frame_id(x); }
    if let Some(x) = args.parent_frame_id { builder.add_parent_frame_id(x); }
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    builder.finish()
  }


  /// Timestamp of transform
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(FrameTransform::VT_TIMESTAMP, None)}
  }
  /// Name of the parent frame
  #[inline]
  pub fn parent_frame_id(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(FrameTransform::VT_PARENT_FRAME_ID, None)}
  }
  /// Name of the child frame
  #[inline]
  pub fn child_frame_id(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(FrameTransform::VT_CHILD_FRAME_ID, None)}
  }
  /// Translation component of the transform, representing the position of the child frame's origin in the parent frame.
  #[inline]
  pub fn translation(&self) -> Option<Vector3<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Vector3>>(FrameTransform::VT_TRANSLATION, None)}
  }
  /// Rotation component of the transform, representing the orientation of the child frame in the parent frame
  #[inline]
  pub fn rotation(&self) -> Option<Quaternion<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Quaternion>>(FrameTransform::VT_ROTATION, None)}
  }
}

impl ::flatbuffers::Verifiable for FrameTransform<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("parent_frame_id", Self::VT_PARENT_FRAME_ID, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("child_frame_id", Self::VT_CHILD_FRAME_ID, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Vector3>>("translation", Self::VT_TRANSLATION, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Quaternion>>("rotation", Self::VT_ROTATION, false)?
     .finish();
    Ok(())
  }
}
pub struct FrameTransformArgs<'a> {
    pub timestamp: Option<&'a Time>,
    pub parent_frame_id: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub child_frame_id: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub translation: Option<::flatbuffers::WIPOffset<Vector3<'a>>>,
    pub rotation: Option<::flatbuffers::WIPOffset<Quaternion<'a>>>,
}
impl<'a> Default for FrameTransformArgs<'a> {
  #[inline]
  fn default() -> Self {
    FrameTransformArgs {
      timestamp: None,
      parent_frame_id: None,
      child_frame_id: None,
      translation: None,
      rotation: None,
    }
  }
}

pub struct FrameTransformBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> FrameTransformBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(FrameTransform::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn add_parent_frame_id(&mut self, parent_frame_id: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(FrameTransform::VT_PARENT_FRAME_ID, parent_frame_id);
  }
  #[inline]
  pub fn add_child_frame_id(&mut self, child_frame_id: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(FrameTransform::VT_CHILD_FRAME_ID, child_frame_id);
  }
  #[inline]
  pub fn add_translation(&mut self, translation: ::flatbuffers::WIPOffset<Vector3<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Vector3>>(FrameTransform::VT_TRANSLATION, translation);
  }
  #[inline]
  pub fn add_rotation(&mut self, rotation: ::flatbuffers::WIPOffset<Quaternion<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Quaternion>>(FrameTransform::VT_ROTATION, rotation);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> FrameTransformBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    FrameTransformBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<FrameTransform<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for FrameTransform<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("FrameTransform");
      ds.field("timestamp", &self.timestamp());
      ds.field("parent_frame_id", &self.parent_frame_id());
      ds.field("child_frame_id", &self.child_frame_id());
      ds.field("translation", &self.translation());
      ds.field("rotation", &self.rotation());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `FrameTransform`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_frame_transform_unchecked`.
pub fn root_as_frame_transform(buf: &[u8]) -> Result<FrameTransform<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<FrameTransform>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `FrameTransform` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_frame_transform_unchecked`.
pub fn size_prefixed_root_as_frame_transform(buf: &[u8]) -> Result<FrameTransform<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<FrameTransform>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `FrameTransform` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_frame_transform_unchecked`.
pub fn root_as_frame_transform_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<FrameTransform<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<FrameTransform<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `FrameTransform` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_frame_transform_unchecked`.
pub fn size_prefixed_root_as_frame_transform_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<FrameTransform<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<FrameTransform<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a FrameTransform and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `FrameTransform`.
pub unsafe fn root_as_frame_transform_unchecked(buf: &[u8]) -> FrameTransform<'_> {
  unsafe { ::flatbuffers::root_unchecked::<FrameTransform>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed FrameTransform and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `FrameTransform`.
pub unsafe fn size_prefixed_root_as_frame_transform_unchecked(buf: &[u8]) -> FrameTransform<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<FrameTransform>(buf) }
}
#[inline]
pub fn finish_frame_transform_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<FrameTransform<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_frame_transform_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<FrameTransform<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from FrameTransforms_generated.rs =====



pub enum FrameTransformsOffset {}
#[derive(Copy, Clone, PartialEq)]

/// An array of FrameTransform messages
pub struct FrameTransforms<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for FrameTransforms<'a> {
  type Inner = FrameTransforms<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> FrameTransforms<'a> {
  pub const VT_TRANSFORMS: ::flatbuffers::VOffsetT = 4;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    FrameTransforms { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args FrameTransformsArgs<'args>
  ) -> ::flatbuffers::WIPOffset<FrameTransforms<'bldr>> {
    let mut builder = FrameTransformsBuilder::new(_fbb);
    if let Some(x) = args.transforms { builder.add_transforms(x); }
    builder.finish()
  }


  /// Array of transforms
  #[inline]
  pub fn transforms(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<FrameTransform<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<FrameTransform>>>>(FrameTransforms::VT_TRANSFORMS, None)}
  }
}

impl ::flatbuffers::Verifiable for FrameTransforms<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<FrameTransform>>>>("transforms", Self::VT_TRANSFORMS, false)?
     .finish();
    Ok(())
  }
}
pub struct FrameTransformsArgs<'a> {
    pub transforms: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<FrameTransform<'a>>>>>,
}
impl<'a> Default for FrameTransformsArgs<'a> {
  #[inline]
  fn default() -> Self {
    FrameTransformsArgs {
      transforms: None,
    }
  }
}

pub struct FrameTransformsBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> FrameTransformsBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_transforms(&mut self, transforms: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<FrameTransform<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(FrameTransforms::VT_TRANSFORMS, transforms);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> FrameTransformsBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    FrameTransformsBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<FrameTransforms<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for FrameTransforms<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("FrameTransforms");
      ds.field("transforms", &self.transforms());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `FrameTransforms`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_frame_transforms_unchecked`.
pub fn root_as_frame_transforms(buf: &[u8]) -> Result<FrameTransforms<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<FrameTransforms>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `FrameTransforms` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_frame_transforms_unchecked`.
pub fn size_prefixed_root_as_frame_transforms(buf: &[u8]) -> Result<FrameTransforms<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<FrameTransforms>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `FrameTransforms` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_frame_transforms_unchecked`.
pub fn root_as_frame_transforms_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<FrameTransforms<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<FrameTransforms<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `FrameTransforms` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_frame_transforms_unchecked`.
pub fn size_prefixed_root_as_frame_transforms_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<FrameTransforms<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<FrameTransforms<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a FrameTransforms and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `FrameTransforms`.
pub unsafe fn root_as_frame_transforms_unchecked(buf: &[u8]) -> FrameTransforms<'_> {
  unsafe { ::flatbuffers::root_unchecked::<FrameTransforms>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed FrameTransforms and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `FrameTransforms`.
pub unsafe fn size_prefixed_root_as_frame_transforms_unchecked(buf: &[u8]) -> FrameTransforms<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<FrameTransforms>(buf) }
}
#[inline]
pub fn finish_frame_transforms_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<FrameTransforms<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_frame_transforms_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<FrameTransforms<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from GeoJSON_generated.rs =====



pub enum GeoJSONOffset {}
#[derive(Copy, Clone, PartialEq)]

/// GeoJSON data for annotating maps
pub struct GeoJSON<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for GeoJSON<'a> {
  type Inner = GeoJSON<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> GeoJSON<'a> {
  pub const VT_GEOJSON: ::flatbuffers::VOffsetT = 4;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    GeoJSON { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args GeoJSONArgs<'args>
  ) -> ::flatbuffers::WIPOffset<GeoJSON<'bldr>> {
    let mut builder = GeoJSONBuilder::new(_fbb);
    if let Some(x) = args.geojson { builder.add_geojson(x); }
    builder.finish()
  }


  /// GeoJSON data encoded as a UTF-8 string
  #[inline]
  pub fn geojson(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(GeoJSON::VT_GEOJSON, None)}
  }
}

impl ::flatbuffers::Verifiable for GeoJSON<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("geojson", Self::VT_GEOJSON, false)?
     .finish();
    Ok(())
  }
}
pub struct GeoJSONArgs<'a> {
    pub geojson: Option<::flatbuffers::WIPOffset<&'a str>>,
}
impl<'a> Default for GeoJSONArgs<'a> {
  #[inline]
  fn default() -> Self {
    GeoJSONArgs {
      geojson: None,
    }
  }
}

pub struct GeoJSONBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> GeoJSONBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_geojson(&mut self, geojson: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(GeoJSON::VT_GEOJSON, geojson);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> GeoJSONBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    GeoJSONBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<GeoJSON<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for GeoJSON<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("GeoJSON");
      ds.field("geojson", &self.geojson());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `GeoJSON`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_geo_json_unchecked`.
pub fn root_as_geo_json(buf: &[u8]) -> Result<GeoJSON<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<GeoJSON>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `GeoJSON` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_geo_json_unchecked`.
pub fn size_prefixed_root_as_geo_json(buf: &[u8]) -> Result<GeoJSON<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<GeoJSON>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `GeoJSON` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_geo_json_unchecked`.
pub fn root_as_geo_json_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<GeoJSON<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<GeoJSON<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `GeoJSON` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_geo_json_unchecked`.
pub fn size_prefixed_root_as_geo_json_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<GeoJSON<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<GeoJSON<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a GeoJSON and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `GeoJSON`.
pub unsafe fn root_as_geo_json_unchecked(buf: &[u8]) -> GeoJSON<'_> {
  unsafe { ::flatbuffers::root_unchecked::<GeoJSON>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed GeoJSON and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `GeoJSON`.
pub unsafe fn size_prefixed_root_as_geo_json_unchecked(buf: &[u8]) -> GeoJSON<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<GeoJSON>(buf) }
}
#[inline]
pub fn finish_geo_json_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<GeoJSON<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_geo_json_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<GeoJSON<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from Grid_generated.rs =====



pub enum GridOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A 2D grid of data
pub struct Grid<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for Grid<'a> {
  type Inner = Grid<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> Grid<'a> {
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
  pub const VT_FRAME_ID: ::flatbuffers::VOffsetT = 6;
  pub const VT_POSE: ::flatbuffers::VOffsetT = 8;
  pub const VT_COLUMN_COUNT: ::flatbuffers::VOffsetT = 10;
  pub const VT_CELL_SIZE: ::flatbuffers::VOffsetT = 12;
  pub const VT_ROW_STRIDE: ::flatbuffers::VOffsetT = 14;
  pub const VT_CELL_STRIDE: ::flatbuffers::VOffsetT = 16;
  pub const VT_FIELDS: ::flatbuffers::VOffsetT = 18;
  pub const VT_DATA: ::flatbuffers::VOffsetT = 20;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    Grid { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args GridArgs<'args>
  ) -> ::flatbuffers::WIPOffset<Grid<'bldr>> {
    let mut builder = GridBuilder::new(_fbb);
    if let Some(x) = args.data { builder.add_data(x); }
    if let Some(x) = args.fields { builder.add_fields(x); }
    builder.add_cell_stride(args.cell_stride);
    builder.add_row_stride(args.row_stride);
    if let Some(x) = args.cell_size { builder.add_cell_size(x); }
    builder.add_column_count(args.column_count);
    if let Some(x) = args.pose { builder.add_pose(x); }
    if let Some(x) = args.frame_id { builder.add_frame_id(x); }
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    builder.finish()
  }


  /// Timestamp of grid
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(Grid::VT_TIMESTAMP, None)}
  }
  /// Frame of reference
  #[inline]
  pub fn frame_id(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(Grid::VT_FRAME_ID, None)}
  }
  /// Origin of grid's corner relative to frame of reference; grid is positioned in the x-y plane relative to this origin
  #[inline]
  pub fn pose(&self) -> Option<Pose<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Pose>>(Grid::VT_POSE, None)}
  }
  /// Number of grid columns
  #[inline]
  pub fn column_count(&self) -> u32 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<u32>(Grid::VT_COLUMN_COUNT, Some(0)).unwrap()}
  }
  /// Size of single grid cell along x and y axes, relative to `pose`
  #[inline]
  pub fn cell_size(&self) -> Option<Vector2<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Vector2>>(Grid::VT_CELL_SIZE, None)}
  }
  /// Number of bytes between rows in `data`
  #[inline]
  pub fn row_stride(&self) -> u32 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<u32>(Grid::VT_ROW_STRIDE, Some(0)).unwrap()}
  }
  /// Number of bytes between cells within a row in `data`
  #[inline]
  pub fn cell_stride(&self) -> u32 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<u32>(Grid::VT_CELL_STRIDE, Some(0)).unwrap()}
  }
  /// Fields in `data`. `red`, `green`, `blue`, and `alpha` are optional for customizing the grid's color.
  /// To enable RGB color visualization in the [3D panel](https://docs.foxglove.dev/docs/visualization/panels/3d#rgba-separate-fields-color-mode), include **all four** of these fields in your `fields` array:
  /// 
  /// - `red` - Red channel value
  /// - `green` - Green channel value
  /// - `blue` - Blue channel value
  /// - `alpha` - Alpha/transparency channel value
  /// 
  /// **note:** All four fields must be present with these exact names for RGB visualization to work. The order of fields doesn't matter, but the names must match exactly.
  /// 
  /// Recommended type: `UINT8` (0-255 range) for standard 8-bit color channels.
  /// 
  /// Example field definitions:
  /// 
  /// **RGB color only:**
  /// 
  /// ```javascript
  /// fields: [
  ///  { name: "red", offset: 0, type: NumericType.UINT8 },
  ///  { name: "green", offset: 1, type: NumericType.UINT8 },
  ///  { name: "blue", offset: 2, type: NumericType.UINT8 },
  ///  { name: "alpha", offset: 3, type: NumericType.UINT8 },
  /// ];
  /// ```
  /// 
  /// **RGB color with elevation (for 3D terrain visualization):**
  /// 
  /// ```javascript
  /// fields: [
  ///  { name: "red", offset: 0, type: NumericType.UINT8 },
  ///  { name: "green", offset: 1, type: NumericType.UINT8 },
  ///  { name: "blue", offset: 2, type: NumericType.UINT8 },
  ///  { name: "alpha", offset: 3, type: NumericType.UINT8 },
  ///  { name: "elevation", offset: 4, type: NumericType.FLOAT32 },
  /// ];
  /// ```
  /// 
  /// When these fields are present, the 3D panel will offer additional "Color Mode" options including "RGBA (separate fields)" to visualize the RGB data directly. For elevation visualization, set the "Elevation field" to your elevation layer name.
  #[inline]
  pub fn fields(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<PackedElementField<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<PackedElementField>>>>(Grid::VT_FIELDS, None)}
  }
  /// Grid cell data, interpreted using `fields`, in row-major (y-major) order.
  /// For the data element starting at byte offset i, the coordinates of its corner closest to the origin will be:
  /// 
  /// - y = i / row_stride * cell_size.y
  /// - x = (i % row_stride) / cell_stride * cell_size.x
  #[inline]
  pub fn data(&self) -> Option<::flatbuffers::Vector<'a, u8>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, u8>>>(Grid::VT_DATA, None)}
  }
}

impl ::flatbuffers::Verifiable for Grid<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("frame_id", Self::VT_FRAME_ID, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Pose>>("pose", Self::VT_POSE, false)?
     .visit_field::<u32>("column_count", Self::VT_COLUMN_COUNT, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Vector2>>("cell_size", Self::VT_CELL_SIZE, false)?
     .visit_field::<u32>("row_stride", Self::VT_ROW_STRIDE, false)?
     .visit_field::<u32>("cell_stride", Self::VT_CELL_STRIDE, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<PackedElementField>>>>("fields", Self::VT_FIELDS, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, u8>>>("data", Self::VT_DATA, false)?
     .finish();
    Ok(())
  }
}
pub struct GridArgs<'a> {
    pub timestamp: Option<&'a Time>,
    pub frame_id: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub pose: Option<::flatbuffers::WIPOffset<Pose<'a>>>,
    pub column_count: u32,
    pub cell_size: Option<::flatbuffers::WIPOffset<Vector2<'a>>>,
    pub row_stride: u32,
    pub cell_stride: u32,
    pub fields: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<PackedElementField<'a>>>>>,
    pub data: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, u8>>>,
}
impl<'a> Default for GridArgs<'a> {
  #[inline]
  fn default() -> Self {
    GridArgs {
      timestamp: None,
      frame_id: None,
      pose: None,
      column_count: 0,
      cell_size: None,
      row_stride: 0,
      cell_stride: 0,
      fields: None,
      data: None,
    }
  }
}

pub struct GridBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> GridBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(Grid::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn add_frame_id(&mut self, frame_id: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(Grid::VT_FRAME_ID, frame_id);
  }
  #[inline]
  pub fn add_pose(&mut self, pose: ::flatbuffers::WIPOffset<Pose<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Pose>>(Grid::VT_POSE, pose);
  }
  #[inline]
  pub fn add_column_count(&mut self, column_count: u32) {
    self.fbb_.push_slot::<u32>(Grid::VT_COLUMN_COUNT, column_count, 0);
  }
  #[inline]
  pub fn add_cell_size(&mut self, cell_size: ::flatbuffers::WIPOffset<Vector2<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Vector2>>(Grid::VT_CELL_SIZE, cell_size);
  }
  #[inline]
  pub fn add_row_stride(&mut self, row_stride: u32) {
    self.fbb_.push_slot::<u32>(Grid::VT_ROW_STRIDE, row_stride, 0);
  }
  #[inline]
  pub fn add_cell_stride(&mut self, cell_stride: u32) {
    self.fbb_.push_slot::<u32>(Grid::VT_CELL_STRIDE, cell_stride, 0);
  }
  #[inline]
  pub fn add_fields(&mut self, fields: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<PackedElementField<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(Grid::VT_FIELDS, fields);
  }
  #[inline]
  pub fn add_data(&mut self, data: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , u8>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(Grid::VT_DATA, data);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> GridBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    GridBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<Grid<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for Grid<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("Grid");
      ds.field("timestamp", &self.timestamp());
      ds.field("frame_id", &self.frame_id());
      ds.field("pose", &self.pose());
      ds.field("column_count", &self.column_count());
      ds.field("cell_size", &self.cell_size());
      ds.field("row_stride", &self.row_stride());
      ds.field("cell_stride", &self.cell_stride());
      ds.field("fields", &self.fields());
      ds.field("data", &self.data());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `Grid`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_grid_unchecked`.
pub fn root_as_grid(buf: &[u8]) -> Result<Grid<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<Grid>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `Grid` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_grid_unchecked`.
pub fn size_prefixed_root_as_grid(buf: &[u8]) -> Result<Grid<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<Grid>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `Grid` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_grid_unchecked`.
pub fn root_as_grid_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<Grid<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<Grid<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `Grid` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_grid_unchecked`.
pub fn size_prefixed_root_as_grid_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<Grid<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<Grid<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a Grid and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `Grid`.
pub unsafe fn root_as_grid_unchecked(buf: &[u8]) -> Grid<'_> {
  unsafe { ::flatbuffers::root_unchecked::<Grid>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed Grid and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `Grid`.
pub unsafe fn size_prefixed_root_as_grid_unchecked(buf: &[u8]) -> Grid<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<Grid>(buf) }
}
#[inline]
pub fn finish_grid_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<Grid<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_grid_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<Grid<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from ImageAnnotations_generated.rs =====



pub enum ImageAnnotationsOffset {}
#[derive(Copy, Clone, PartialEq)]

/// Array of annotations for a 2D image
pub struct ImageAnnotations<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for ImageAnnotations<'a> {
  type Inner = ImageAnnotations<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> ImageAnnotations<'a> {
  pub const VT_CIRCLES: ::flatbuffers::VOffsetT = 4;
  pub const VT_POINTS: ::flatbuffers::VOffsetT = 6;
  pub const VT_TEXTS: ::flatbuffers::VOffsetT = 8;
  pub const VT_METADATA: ::flatbuffers::VOffsetT = 10;
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 12;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    ImageAnnotations { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args ImageAnnotationsArgs<'args>
  ) -> ::flatbuffers::WIPOffset<ImageAnnotations<'bldr>> {
    let mut builder = ImageAnnotationsBuilder::new(_fbb);
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    if let Some(x) = args.metadata { builder.add_metadata(x); }
    if let Some(x) = args.texts { builder.add_texts(x); }
    if let Some(x) = args.points { builder.add_points(x); }
    if let Some(x) = args.circles { builder.add_circles(x); }
    builder.finish()
  }


  /// Circle annotations
  #[inline]
  pub fn circles(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<CircleAnnotation<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<CircleAnnotation>>>>(ImageAnnotations::VT_CIRCLES, None)}
  }
  /// Points annotations
  #[inline]
  pub fn points(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<PointsAnnotation<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<PointsAnnotation>>>>(ImageAnnotations::VT_POINTS, None)}
  }
  /// Text annotations
  #[inline]
  pub fn texts(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<TextAnnotation<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<TextAnnotation>>>>(ImageAnnotations::VT_TEXTS, None)}
  }
  /// Additional user-provided metadata associated with the image annotations. Keys must be unique within this object. Per-annotation metadata takes precedence over these values.
  #[inline]
  pub fn metadata(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<KeyValuePair<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<KeyValuePair>>>>(ImageAnnotations::VT_METADATA, None)}
  }
  /// Timestamp of the image annotations. When set, individual annotation timestamps will be ignored.
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(ImageAnnotations::VT_TIMESTAMP, None)}
  }
}

impl ::flatbuffers::Verifiable for ImageAnnotations<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<CircleAnnotation>>>>("circles", Self::VT_CIRCLES, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<PointsAnnotation>>>>("points", Self::VT_POINTS, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<TextAnnotation>>>>("texts", Self::VT_TEXTS, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<KeyValuePair>>>>("metadata", Self::VT_METADATA, false)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .finish();
    Ok(())
  }
}
pub struct ImageAnnotationsArgs<'a> {
    pub circles: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<CircleAnnotation<'a>>>>>,
    pub points: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<PointsAnnotation<'a>>>>>,
    pub texts: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<TextAnnotation<'a>>>>>,
    pub metadata: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<KeyValuePair<'a>>>>>,
    pub timestamp: Option<&'a Time>,
}
impl<'a> Default for ImageAnnotationsArgs<'a> {
  #[inline]
  fn default() -> Self {
    ImageAnnotationsArgs {
      circles: None,
      points: None,
      texts: None,
      metadata: None,
      timestamp: None,
    }
  }
}

pub struct ImageAnnotationsBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> ImageAnnotationsBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_circles(&mut self, circles: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<CircleAnnotation<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(ImageAnnotations::VT_CIRCLES, circles);
  }
  #[inline]
  pub fn add_points(&mut self, points: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<PointsAnnotation<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(ImageAnnotations::VT_POINTS, points);
  }
  #[inline]
  pub fn add_texts(&mut self, texts: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<TextAnnotation<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(ImageAnnotations::VT_TEXTS, texts);
  }
  #[inline]
  pub fn add_metadata(&mut self, metadata: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<KeyValuePair<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(ImageAnnotations::VT_METADATA, metadata);
  }
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(ImageAnnotations::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> ImageAnnotationsBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    ImageAnnotationsBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<ImageAnnotations<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for ImageAnnotations<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("ImageAnnotations");
      ds.field("circles", &self.circles());
      ds.field("points", &self.points());
      ds.field("texts", &self.texts());
      ds.field("metadata", &self.metadata());
      ds.field("timestamp", &self.timestamp());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `ImageAnnotations`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_image_annotations_unchecked`.
pub fn root_as_image_annotations(buf: &[u8]) -> Result<ImageAnnotations<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<ImageAnnotations>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `ImageAnnotations` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_image_annotations_unchecked`.
pub fn size_prefixed_root_as_image_annotations(buf: &[u8]) -> Result<ImageAnnotations<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<ImageAnnotations>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `ImageAnnotations` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_image_annotations_unchecked`.
pub fn root_as_image_annotations_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<ImageAnnotations<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<ImageAnnotations<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `ImageAnnotations` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_image_annotations_unchecked`.
pub fn size_prefixed_root_as_image_annotations_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<ImageAnnotations<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<ImageAnnotations<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a ImageAnnotations and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `ImageAnnotations`.
pub unsafe fn root_as_image_annotations_unchecked(buf: &[u8]) -> ImageAnnotations<'_> {
  unsafe { ::flatbuffers::root_unchecked::<ImageAnnotations>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed ImageAnnotations and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `ImageAnnotations`.
pub unsafe fn size_prefixed_root_as_image_annotations_unchecked(buf: &[u8]) -> ImageAnnotations<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<ImageAnnotations>(buf) }
}
#[inline]
pub fn finish_image_annotations_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<ImageAnnotations<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_image_annotations_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<ImageAnnotations<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from JointState_generated.rs =====



pub enum JointStateOffset {}
#[derive(Copy, Clone, PartialEq)]

/// The state of a single joint (revolute or prismatic).
pub struct JointState<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for JointState<'a> {
  type Inner = JointState<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> JointState<'a> {
  pub const VT_NAME: ::flatbuffers::VOffsetT = 4;
  pub const VT_POSITION: ::flatbuffers::VOffsetT = 6;
  pub const VT_VELOCITY: ::flatbuffers::VOffsetT = 8;
  pub const VT_ACCELERATION: ::flatbuffers::VOffsetT = 10;
  pub const VT_EFFORT: ::flatbuffers::VOffsetT = 12;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    JointState { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args JointStateArgs<'args>
  ) -> ::flatbuffers::WIPOffset<JointState<'bldr>> {
    let mut builder = JointStateBuilder::new(_fbb);
    if let Some(x) = args.effort { builder.add_effort(x); }
    if let Some(x) = args.acceleration { builder.add_acceleration(x); }
    if let Some(x) = args.velocity { builder.add_velocity(x); }
    if let Some(x) = args.position { builder.add_position(x); }
    if let Some(x) = args.name { builder.add_name(x); }
    builder.finish()
  }


  /// Joint name
  #[inline]
  pub fn name(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(JointState::VT_NAME, None)}
  }
  /// Joint position. Radians for revolute joints, meters for prismatic joints.
  #[inline]
  pub fn position(&self) -> Option<f64> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(JointState::VT_POSITION, None)}
  }
  /// Joint velocity. Rad/s for revolute joints, m/s for prismatic joints.
  #[inline]
  pub fn velocity(&self) -> Option<f64> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(JointState::VT_VELOCITY, None)}
  }
  /// Joint acceleration. Rad/s² for revolute joints, m/s² for prismatic joints.
  #[inline]
  pub fn acceleration(&self) -> Option<f64> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(JointState::VT_ACCELERATION, None)}
  }
  /// Joint effort (force or torque). Nm for revolute joints, N for prismatic joints.
  #[inline]
  pub fn effort(&self) -> Option<f64> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(JointState::VT_EFFORT, None)}
  }
}

impl ::flatbuffers::Verifiable for JointState<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("name", Self::VT_NAME, false)?
     .visit_field::<f64>("position", Self::VT_POSITION, false)?
     .visit_field::<f64>("velocity", Self::VT_VELOCITY, false)?
     .visit_field::<f64>("acceleration", Self::VT_ACCELERATION, false)?
     .visit_field::<f64>("effort", Self::VT_EFFORT, false)?
     .finish();
    Ok(())
  }
}
pub struct JointStateArgs<'a> {
    pub name: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub position: Option<f64>,
    pub velocity: Option<f64>,
    pub acceleration: Option<f64>,
    pub effort: Option<f64>,
}
impl<'a> Default for JointStateArgs<'a> {
  #[inline]
  fn default() -> Self {
    JointStateArgs {
      name: None,
      position: None,
      velocity: None,
      acceleration: None,
      effort: None,
    }
  }
}

pub struct JointStateBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> JointStateBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_name(&mut self, name: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(JointState::VT_NAME, name);
  }
  #[inline]
  pub fn add_position(&mut self, position: f64) {
    self.fbb_.push_slot_always::<f64>(JointState::VT_POSITION, position);
  }
  #[inline]
  pub fn add_velocity(&mut self, velocity: f64) {
    self.fbb_.push_slot_always::<f64>(JointState::VT_VELOCITY, velocity);
  }
  #[inline]
  pub fn add_acceleration(&mut self, acceleration: f64) {
    self.fbb_.push_slot_always::<f64>(JointState::VT_ACCELERATION, acceleration);
  }
  #[inline]
  pub fn add_effort(&mut self, effort: f64) {
    self.fbb_.push_slot_always::<f64>(JointState::VT_EFFORT, effort);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> JointStateBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    JointStateBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<JointState<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for JointState<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("JointState");
      ds.field("name", &self.name());
      ds.field("position", &self.position());
      ds.field("velocity", &self.velocity());
      ds.field("acceleration", &self.acceleration());
      ds.field("effort", &self.effort());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `JointState`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_joint_state_unchecked`.
pub fn root_as_joint_state(buf: &[u8]) -> Result<JointState<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<JointState>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `JointState` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_joint_state_unchecked`.
pub fn size_prefixed_root_as_joint_state(buf: &[u8]) -> Result<JointState<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<JointState>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `JointState` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_joint_state_unchecked`.
pub fn root_as_joint_state_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<JointState<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<JointState<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `JointState` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_joint_state_unchecked`.
pub fn size_prefixed_root_as_joint_state_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<JointState<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<JointState<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a JointState and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `JointState`.
pub unsafe fn root_as_joint_state_unchecked(buf: &[u8]) -> JointState<'_> {
  unsafe { ::flatbuffers::root_unchecked::<JointState>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed JointState and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `JointState`.
pub unsafe fn size_prefixed_root_as_joint_state_unchecked(buf: &[u8]) -> JointState<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<JointState>(buf) }
}
#[inline]
pub fn finish_joint_state_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<JointState<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_joint_state_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<JointState<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from JointStates_generated.rs =====



pub enum JointStatesOffset {}
#[derive(Copy, Clone, PartialEq)]

/// The state of a set of joints at a given time.
pub struct JointStates<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for JointStates<'a> {
  type Inner = JointStates<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> JointStates<'a> {
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
  pub const VT_JOINTS: ::flatbuffers::VOffsetT = 6;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    JointStates { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args JointStatesArgs<'args>
  ) -> ::flatbuffers::WIPOffset<JointStates<'bldr>> {
    let mut builder = JointStatesBuilder::new(_fbb);
    if let Some(x) = args.joints { builder.add_joints(x); }
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    builder.finish()
  }


  /// Timestamp of the joint states
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(JointStates::VT_TIMESTAMP, None)}
  }
  /// Joint states
  #[inline]
  pub fn joints(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<JointState<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<JointState>>>>(JointStates::VT_JOINTS, None)}
  }
}

impl ::flatbuffers::Verifiable for JointStates<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<JointState>>>>("joints", Self::VT_JOINTS, false)?
     .finish();
    Ok(())
  }
}
pub struct JointStatesArgs<'a> {
    pub timestamp: Option<&'a Time>,
    pub joints: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<JointState<'a>>>>>,
}
impl<'a> Default for JointStatesArgs<'a> {
  #[inline]
  fn default() -> Self {
    JointStatesArgs {
      timestamp: None,
      joints: None,
    }
  }
}

pub struct JointStatesBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> JointStatesBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(JointStates::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn add_joints(&mut self, joints: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<JointState<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(JointStates::VT_JOINTS, joints);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> JointStatesBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    JointStatesBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<JointStates<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for JointStates<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("JointStates");
      ds.field("timestamp", &self.timestamp());
      ds.field("joints", &self.joints());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `JointStates`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_joint_states_unchecked`.
pub fn root_as_joint_states(buf: &[u8]) -> Result<JointStates<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<JointStates>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `JointStates` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_joint_states_unchecked`.
pub fn size_prefixed_root_as_joint_states(buf: &[u8]) -> Result<JointStates<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<JointStates>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `JointStates` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_joint_states_unchecked`.
pub fn root_as_joint_states_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<JointStates<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<JointStates<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `JointStates` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_joint_states_unchecked`.
pub fn size_prefixed_root_as_joint_states_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<JointStates<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<JointStates<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a JointStates and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `JointStates`.
pub unsafe fn root_as_joint_states_unchecked(buf: &[u8]) -> JointStates<'_> {
  unsafe { ::flatbuffers::root_unchecked::<JointStates>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed JointStates and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `JointStates`.
pub unsafe fn size_prefixed_root_as_joint_states_unchecked(buf: &[u8]) -> JointStates<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<JointStates>(buf) }
}
#[inline]
pub fn finish_joint_states_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<JointStates<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_joint_states_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<JointStates<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from KeyValuePair_generated.rs =====



pub enum KeyValuePairOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A key with its associated value
pub struct KeyValuePair<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for KeyValuePair<'a> {
  type Inner = KeyValuePair<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> KeyValuePair<'a> {
  pub const VT_KEY: ::flatbuffers::VOffsetT = 4;
  pub const VT_VALUE: ::flatbuffers::VOffsetT = 6;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    KeyValuePair { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args KeyValuePairArgs<'args>
  ) -> ::flatbuffers::WIPOffset<KeyValuePair<'bldr>> {
    let mut builder = KeyValuePairBuilder::new(_fbb);
    if let Some(x) = args.value { builder.add_value(x); }
    if let Some(x) = args.key { builder.add_key(x); }
    builder.finish()
  }


  /// Key
  #[inline]
  pub fn key(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(KeyValuePair::VT_KEY, None)}
  }
  /// Value
  #[inline]
  pub fn value(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(KeyValuePair::VT_VALUE, None)}
  }
}

impl ::flatbuffers::Verifiable for KeyValuePair<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("key", Self::VT_KEY, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("value", Self::VT_VALUE, false)?
     .finish();
    Ok(())
  }
}
pub struct KeyValuePairArgs<'a> {
    pub key: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub value: Option<::flatbuffers::WIPOffset<&'a str>>,
}
impl<'a> Default for KeyValuePairArgs<'a> {
  #[inline]
  fn default() -> Self {
    KeyValuePairArgs {
      key: None,
      value: None,
    }
  }
}

pub struct KeyValuePairBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> KeyValuePairBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_key(&mut self, key: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(KeyValuePair::VT_KEY, key);
  }
  #[inline]
  pub fn add_value(&mut self, value: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(KeyValuePair::VT_VALUE, value);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> KeyValuePairBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    KeyValuePairBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<KeyValuePair<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for KeyValuePair<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("KeyValuePair");
      ds.field("key", &self.key());
      ds.field("value", &self.value());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `KeyValuePair`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_key_value_pair_unchecked`.
pub fn root_as_key_value_pair(buf: &[u8]) -> Result<KeyValuePair<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<KeyValuePair>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `KeyValuePair` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_key_value_pair_unchecked`.
pub fn size_prefixed_root_as_key_value_pair(buf: &[u8]) -> Result<KeyValuePair<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<KeyValuePair>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `KeyValuePair` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_key_value_pair_unchecked`.
pub fn root_as_key_value_pair_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<KeyValuePair<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<KeyValuePair<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `KeyValuePair` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_key_value_pair_unchecked`.
pub fn size_prefixed_root_as_key_value_pair_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<KeyValuePair<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<KeyValuePair<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a KeyValuePair and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `KeyValuePair`.
pub unsafe fn root_as_key_value_pair_unchecked(buf: &[u8]) -> KeyValuePair<'_> {
  unsafe { ::flatbuffers::root_unchecked::<KeyValuePair>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed KeyValuePair and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `KeyValuePair`.
pub unsafe fn size_prefixed_root_as_key_value_pair_unchecked(buf: &[u8]) -> KeyValuePair<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<KeyValuePair>(buf) }
}
#[inline]
pub fn finish_key_value_pair_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<KeyValuePair<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_key_value_pair_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<KeyValuePair<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from LaserScan_generated.rs =====



pub enum LaserScanOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A single scan from a planar laser range-finder
pub struct LaserScan<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for LaserScan<'a> {
  type Inner = LaserScan<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> LaserScan<'a> {
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
  pub const VT_FRAME_ID: ::flatbuffers::VOffsetT = 6;
  pub const VT_POSE: ::flatbuffers::VOffsetT = 8;
  pub const VT_START_ANGLE: ::flatbuffers::VOffsetT = 10;
  pub const VT_END_ANGLE: ::flatbuffers::VOffsetT = 12;
  pub const VT_RANGES: ::flatbuffers::VOffsetT = 14;
  pub const VT_INTENSITIES: ::flatbuffers::VOffsetT = 16;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    LaserScan { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args LaserScanArgs<'args>
  ) -> ::flatbuffers::WIPOffset<LaserScan<'bldr>> {
    let mut builder = LaserScanBuilder::new(_fbb);
    builder.add_end_angle(args.end_angle);
    builder.add_start_angle(args.start_angle);
    if let Some(x) = args.intensities { builder.add_intensities(x); }
    if let Some(x) = args.ranges { builder.add_ranges(x); }
    if let Some(x) = args.pose { builder.add_pose(x); }
    if let Some(x) = args.frame_id { builder.add_frame_id(x); }
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    builder.finish()
  }


  /// Timestamp of scan
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(LaserScan::VT_TIMESTAMP, None)}
  }
  /// Frame of reference
  #[inline]
  pub fn frame_id(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(LaserScan::VT_FRAME_ID, None)}
  }
  /// Origin of scan relative to frame of reference; points are positioned in the x-y plane relative to this origin; angles are interpreted as counterclockwise rotations around the z axis with 0 rad being in the +x direction
  #[inline]
  pub fn pose(&self) -> Option<Pose<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Pose>>(LaserScan::VT_POSE, None)}
  }
  /// Bearing of first point, in radians
  #[inline]
  pub fn start_angle(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(LaserScan::VT_START_ANGLE, Some(0.0)).unwrap()}
  }
  /// Bearing of last point, in radians
  #[inline]
  pub fn end_angle(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(LaserScan::VT_END_ANGLE, Some(0.0)).unwrap()}
  }
  /// Distance of detections from origin; assumed to be at equally-spaced angles between `start_angle` and `end_angle`
  #[inline]
  pub fn ranges(&self) -> Option<::flatbuffers::Vector<'a, f64>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, f64>>>(LaserScan::VT_RANGES, None)}
  }
  /// Intensity of detections
  #[inline]
  pub fn intensities(&self) -> Option<::flatbuffers::Vector<'a, f64>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, f64>>>(LaserScan::VT_INTENSITIES, None)}
  }
}

impl ::flatbuffers::Verifiable for LaserScan<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("frame_id", Self::VT_FRAME_ID, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Pose>>("pose", Self::VT_POSE, false)?
     .visit_field::<f64>("start_angle", Self::VT_START_ANGLE, false)?
     .visit_field::<f64>("end_angle", Self::VT_END_ANGLE, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, f64>>>("ranges", Self::VT_RANGES, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, f64>>>("intensities", Self::VT_INTENSITIES, false)?
     .finish();
    Ok(())
  }
}
pub struct LaserScanArgs<'a> {
    pub timestamp: Option<&'a Time>,
    pub frame_id: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub pose: Option<::flatbuffers::WIPOffset<Pose<'a>>>,
    pub start_angle: f64,
    pub end_angle: f64,
    pub ranges: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, f64>>>,
    pub intensities: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, f64>>>,
}
impl<'a> Default for LaserScanArgs<'a> {
  #[inline]
  fn default() -> Self {
    LaserScanArgs {
      timestamp: None,
      frame_id: None,
      pose: None,
      start_angle: 0.0,
      end_angle: 0.0,
      ranges: None,
      intensities: None,
    }
  }
}

pub struct LaserScanBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> LaserScanBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(LaserScan::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn add_frame_id(&mut self, frame_id: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(LaserScan::VT_FRAME_ID, frame_id);
  }
  #[inline]
  pub fn add_pose(&mut self, pose: ::flatbuffers::WIPOffset<Pose<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Pose>>(LaserScan::VT_POSE, pose);
  }
  #[inline]
  pub fn add_start_angle(&mut self, start_angle: f64) {
    self.fbb_.push_slot::<f64>(LaserScan::VT_START_ANGLE, start_angle, 0.0);
  }
  #[inline]
  pub fn add_end_angle(&mut self, end_angle: f64) {
    self.fbb_.push_slot::<f64>(LaserScan::VT_END_ANGLE, end_angle, 0.0);
  }
  #[inline]
  pub fn add_ranges(&mut self, ranges: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , f64>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(LaserScan::VT_RANGES, ranges);
  }
  #[inline]
  pub fn add_intensities(&mut self, intensities: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , f64>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(LaserScan::VT_INTENSITIES, intensities);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> LaserScanBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    LaserScanBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<LaserScan<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for LaserScan<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("LaserScan");
      ds.field("timestamp", &self.timestamp());
      ds.field("frame_id", &self.frame_id());
      ds.field("pose", &self.pose());
      ds.field("start_angle", &self.start_angle());
      ds.field("end_angle", &self.end_angle());
      ds.field("ranges", &self.ranges());
      ds.field("intensities", &self.intensities());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `LaserScan`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_laser_scan_unchecked`.
pub fn root_as_laser_scan(buf: &[u8]) -> Result<LaserScan<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<LaserScan>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `LaserScan` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_laser_scan_unchecked`.
pub fn size_prefixed_root_as_laser_scan(buf: &[u8]) -> Result<LaserScan<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<LaserScan>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `LaserScan` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_laser_scan_unchecked`.
pub fn root_as_laser_scan_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<LaserScan<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<LaserScan<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `LaserScan` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_laser_scan_unchecked`.
pub fn size_prefixed_root_as_laser_scan_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<LaserScan<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<LaserScan<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a LaserScan and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `LaserScan`.
pub unsafe fn root_as_laser_scan_unchecked(buf: &[u8]) -> LaserScan<'_> {
  unsafe { ::flatbuffers::root_unchecked::<LaserScan>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed LaserScan and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `LaserScan`.
pub unsafe fn size_prefixed_root_as_laser_scan_unchecked(buf: &[u8]) -> LaserScan<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<LaserScan>(buf) }
}
#[inline]
pub fn finish_laser_scan_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<LaserScan<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_laser_scan_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<LaserScan<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from LinePrimitive_generated.rs =====



#[deprecated(since = "2.0.0", note = "Use associated constants instead. This will no longer be generated in 2021.")]
pub const ENUM_MIN_LINE_TYPE: u8 = 0;
#[deprecated(since = "2.0.0", note = "Use associated constants instead. This will no longer be generated in 2021.")]
pub const ENUM_MAX_LINE_TYPE: u8 = 2;
#[deprecated(since = "2.0.0", note = "Use associated constants instead. This will no longer be generated in 2021.")]
#[allow(non_camel_case_types)]
pub const ENUM_VALUES_LINE_TYPE: [LineType; 3] = [
  LineType::LINE_STRIP,
  LineType::LINE_LOOP,
  LineType::LINE_LIST,
];

/// An enumeration indicating how input points should be interpreted to create lines
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct LineType(pub u8);
#[allow(non_upper_case_globals)]
impl LineType {
  /// Connected line segments: 0-1, 1-2, ..., (n-1)-n
  pub const LINE_STRIP: Self = Self(0);
  /// Closed polygon: 0-1, 1-2, ..., (n-1)-n, n-0
  pub const LINE_LOOP: Self = Self(1);
  /// Individual line segments: 0-1, 2-3, 4-5, ...
  pub const LINE_LIST: Self = Self(2);

  pub const ENUM_MIN: u8 = 0;
  pub const ENUM_MAX: u8 = 2;
  pub const ENUM_VALUES: &'static [Self] = &[
    Self::LINE_STRIP,
    Self::LINE_LOOP,
    Self::LINE_LIST,
  ];
  /// Returns the variant's name or "" if unknown.
  pub fn variant_name(self) -> Option<&'static str> {
    match self {
      Self::LINE_STRIP => Some("LINE_STRIP"),
      Self::LINE_LOOP => Some("LINE_LOOP"),
      Self::LINE_LIST => Some("LINE_LIST"),
      _ => None,
    }
  }
}
impl ::core::fmt::Debug for LineType {
  fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
    if let Some(name) = self.variant_name() {
      f.write_str(name)
    } else {
      f.write_fmt(format_args!("<UNKNOWN {:?}>", self.0))
    }
  }
}
impl<'a> ::flatbuffers::Follow<'a> for LineType {
  type Inner = Self;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    let b = unsafe { ::flatbuffers::read_scalar_at::<u8>(buf, loc) };
    Self(b)
  }
}

impl ::flatbuffers::Push for LineType {
    type Output = LineType;
    #[inline]
    unsafe fn push(&self, dst: &mut [u8], _written_len: usize) {
        unsafe { ::flatbuffers::emplace_scalar::<u8>(dst, self.0) };
    }
}

impl ::flatbuffers::EndianScalar for LineType {
  type Scalar = u8;
  #[inline]
  fn to_little_endian(self) -> u8 {
    self.0.to_le()
  }
  #[inline]
  #[allow(clippy::wrong_self_convention)]
  fn from_little_endian(v: u8) -> Self {
    let b = u8::from_le(v);
    Self(b)
  }
}

impl<'a> ::flatbuffers::Verifiable for LineType {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    u8::run_verifier(v, pos)
  }
}

impl ::flatbuffers::SimpleToVerifyInSlice for LineType {}
pub enum LinePrimitiveOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A primitive representing a series of points connected by lines
pub struct LinePrimitive<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for LinePrimitive<'a> {
  type Inner = LinePrimitive<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> LinePrimitive<'a> {
  pub const VT_TYPE_: ::flatbuffers::VOffsetT = 4;
  pub const VT_POSE: ::flatbuffers::VOffsetT = 6;
  pub const VT_THICKNESS: ::flatbuffers::VOffsetT = 8;
  pub const VT_SCALE_INVARIANT: ::flatbuffers::VOffsetT = 10;
  pub const VT_POINTS: ::flatbuffers::VOffsetT = 12;
  pub const VT_COLOR: ::flatbuffers::VOffsetT = 14;
  pub const VT_COLORS: ::flatbuffers::VOffsetT = 16;
  pub const VT_INDICES: ::flatbuffers::VOffsetT = 18;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    LinePrimitive { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args LinePrimitiveArgs<'args>
  ) -> ::flatbuffers::WIPOffset<LinePrimitive<'bldr>> {
    let mut builder = LinePrimitiveBuilder::new(_fbb);
    builder.add_thickness(args.thickness);
    if let Some(x) = args.indices { builder.add_indices(x); }
    if let Some(x) = args.colors { builder.add_colors(x); }
    if let Some(x) = args.color { builder.add_color(x); }
    if let Some(x) = args.points { builder.add_points(x); }
    if let Some(x) = args.pose { builder.add_pose(x); }
    builder.add_scale_invariant(args.scale_invariant);
    builder.add_type_(args.type_);
    builder.finish()
  }


  /// Drawing primitive to use for lines
  #[inline]
  pub fn type_(&self) -> LineType {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<LineType>(LinePrimitive::VT_TYPE_, Some(LineType::LINE_STRIP)).unwrap()}
  }
  /// Origin of lines relative to reference frame
  #[inline]
  pub fn pose(&self) -> Option<Pose<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Pose>>(LinePrimitive::VT_POSE, None)}
  }
  /// Line thickness
  #[inline]
  pub fn thickness(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(LinePrimitive::VT_THICKNESS, Some(0.0)).unwrap()}
  }
  /// Indicates whether `thickness` is a fixed size in screen pixels (true), or specified in world coordinates and scales with distance from the camera (false)
  #[inline]
  pub fn scale_invariant(&self) -> bool {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<bool>(LinePrimitive::VT_SCALE_INVARIANT, Some(false)).unwrap()}
  }
  /// Points along the line
  #[inline]
  pub fn points(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<Point3<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<Point3>>>>(LinePrimitive::VT_POINTS, None)}
  }
  /// Solid color to use for the whole line. Ignored if `colors` is non-empty.
  #[inline]
  pub fn color(&self) -> Option<Color<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Color>>(LinePrimitive::VT_COLOR, None)}
  }
  /// Per-point colors (if non-empty, must have the same length as `points`).
  #[inline]
  pub fn colors(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<Color<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<Color>>>>(LinePrimitive::VT_COLORS, None)}
  }
  /// Indices into the `points` and `colors` attribute arrays, which can be used to avoid duplicating attribute data.
  /// 
  /// If omitted or empty, indexing will not be used. This default behavior is equivalent to specifying [0, 1, ..., N-1] for the indices (where N is the number of `points` provided).
  #[inline]
  pub fn indices(&self) -> Option<::flatbuffers::Vector<'a, u32>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, u32>>>(LinePrimitive::VT_INDICES, None)}
  }
}

impl ::flatbuffers::Verifiable for LinePrimitive<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<LineType>("type_", Self::VT_TYPE_, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Pose>>("pose", Self::VT_POSE, false)?
     .visit_field::<f64>("thickness", Self::VT_THICKNESS, false)?
     .visit_field::<bool>("scale_invariant", Self::VT_SCALE_INVARIANT, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<Point3>>>>("points", Self::VT_POINTS, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Color>>("color", Self::VT_COLOR, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<Color>>>>("colors", Self::VT_COLORS, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, u32>>>("indices", Self::VT_INDICES, false)?
     .finish();
    Ok(())
  }
}
pub struct LinePrimitiveArgs<'a> {
    pub type_: LineType,
    pub pose: Option<::flatbuffers::WIPOffset<Pose<'a>>>,
    pub thickness: f64,
    pub scale_invariant: bool,
    pub points: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<Point3<'a>>>>>,
    pub color: Option<::flatbuffers::WIPOffset<Color<'a>>>,
    pub colors: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<Color<'a>>>>>,
    pub indices: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, u32>>>,
}
impl<'a> Default for LinePrimitiveArgs<'a> {
  #[inline]
  fn default() -> Self {
    LinePrimitiveArgs {
      type_: LineType::LINE_STRIP,
      pose: None,
      thickness: 0.0,
      scale_invariant: false,
      points: None,
      color: None,
      colors: None,
      indices: None,
    }
  }
}

pub struct LinePrimitiveBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> LinePrimitiveBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_type_(&mut self, type_: LineType) {
    self.fbb_.push_slot::<LineType>(LinePrimitive::VT_TYPE_, type_, LineType::LINE_STRIP);
  }
  #[inline]
  pub fn add_pose(&mut self, pose: ::flatbuffers::WIPOffset<Pose<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Pose>>(LinePrimitive::VT_POSE, pose);
  }
  #[inline]
  pub fn add_thickness(&mut self, thickness: f64) {
    self.fbb_.push_slot::<f64>(LinePrimitive::VT_THICKNESS, thickness, 0.0);
  }
  #[inline]
  pub fn add_scale_invariant(&mut self, scale_invariant: bool) {
    self.fbb_.push_slot::<bool>(LinePrimitive::VT_SCALE_INVARIANT, scale_invariant, false);
  }
  #[inline]
  pub fn add_points(&mut self, points: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<Point3<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(LinePrimitive::VT_POINTS, points);
  }
  #[inline]
  pub fn add_color(&mut self, color: ::flatbuffers::WIPOffset<Color<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Color>>(LinePrimitive::VT_COLOR, color);
  }
  #[inline]
  pub fn add_colors(&mut self, colors: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<Color<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(LinePrimitive::VT_COLORS, colors);
  }
  #[inline]
  pub fn add_indices(&mut self, indices: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , u32>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(LinePrimitive::VT_INDICES, indices);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> LinePrimitiveBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    LinePrimitiveBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<LinePrimitive<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for LinePrimitive<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("LinePrimitive");
      ds.field("type_", &self.type_());
      ds.field("pose", &self.pose());
      ds.field("thickness", &self.thickness());
      ds.field("scale_invariant", &self.scale_invariant());
      ds.field("points", &self.points());
      ds.field("color", &self.color());
      ds.field("colors", &self.colors());
      ds.field("indices", &self.indices());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `LinePrimitive`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_line_primitive_unchecked`.
pub fn root_as_line_primitive(buf: &[u8]) -> Result<LinePrimitive<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<LinePrimitive>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `LinePrimitive` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_line_primitive_unchecked`.
pub fn size_prefixed_root_as_line_primitive(buf: &[u8]) -> Result<LinePrimitive<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<LinePrimitive>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `LinePrimitive` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_line_primitive_unchecked`.
pub fn root_as_line_primitive_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<LinePrimitive<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<LinePrimitive<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `LinePrimitive` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_line_primitive_unchecked`.
pub fn size_prefixed_root_as_line_primitive_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<LinePrimitive<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<LinePrimitive<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a LinePrimitive and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `LinePrimitive`.
pub unsafe fn root_as_line_primitive_unchecked(buf: &[u8]) -> LinePrimitive<'_> {
  unsafe { ::flatbuffers::root_unchecked::<LinePrimitive>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed LinePrimitive and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `LinePrimitive`.
pub unsafe fn size_prefixed_root_as_line_primitive_unchecked(buf: &[u8]) -> LinePrimitive<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<LinePrimitive>(buf) }
}
#[inline]
pub fn finish_line_primitive_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<LinePrimitive<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_line_primitive_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<LinePrimitive<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from LocationFix_generated.rs =====



#[deprecated(since = "2.0.0", note = "Use associated constants instead. This will no longer be generated in 2021.")]
pub const ENUM_MIN_POSITION_COVARIANCE_TYPE: u8 = 0;
#[deprecated(since = "2.0.0", note = "Use associated constants instead. This will no longer be generated in 2021.")]
pub const ENUM_MAX_POSITION_COVARIANCE_TYPE: u8 = 3;
#[deprecated(since = "2.0.0", note = "Use associated constants instead. This will no longer be generated in 2021.")]
#[allow(non_camel_case_types)]
pub const ENUM_VALUES_POSITION_COVARIANCE_TYPE: [PositionCovarianceType; 4] = [
  PositionCovarianceType::UNKNOWN,
  PositionCovarianceType::APPROXIMATED,
  PositionCovarianceType::DIAGONAL_KNOWN,
  PositionCovarianceType::KNOWN,
];

/// Type of position covariance
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct PositionCovarianceType(pub u8);
#[allow(non_upper_case_globals)]
impl PositionCovarianceType {
  /// Unknown position covariance type
  pub const UNKNOWN: Self = Self(0);
  /// Position covariance is approximated
  pub const APPROXIMATED: Self = Self(1);
  /// Position covariance is per-axis, so put it along the diagonal
  pub const DIAGONAL_KNOWN: Self = Self(2);
  /// Position covariance of the fix is known
  pub const KNOWN: Self = Self(3);

  pub const ENUM_MIN: u8 = 0;
  pub const ENUM_MAX: u8 = 3;
  pub const ENUM_VALUES: &'static [Self] = &[
    Self::UNKNOWN,
    Self::APPROXIMATED,
    Self::DIAGONAL_KNOWN,
    Self::KNOWN,
  ];
  /// Returns the variant's name or "" if unknown.
  pub fn variant_name(self) -> Option<&'static str> {
    match self {
      Self::UNKNOWN => Some("UNKNOWN"),
      Self::APPROXIMATED => Some("APPROXIMATED"),
      Self::DIAGONAL_KNOWN => Some("DIAGONAL_KNOWN"),
      Self::KNOWN => Some("KNOWN"),
      _ => None,
    }
  }
}
impl ::core::fmt::Debug for PositionCovarianceType {
  fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
    if let Some(name) = self.variant_name() {
      f.write_str(name)
    } else {
      f.write_fmt(format_args!("<UNKNOWN {:?}>", self.0))
    }
  }
}
impl<'a> ::flatbuffers::Follow<'a> for PositionCovarianceType {
  type Inner = Self;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    let b = unsafe { ::flatbuffers::read_scalar_at::<u8>(buf, loc) };
    Self(b)
  }
}

impl ::flatbuffers::Push for PositionCovarianceType {
    type Output = PositionCovarianceType;
    #[inline]
    unsafe fn push(&self, dst: &mut [u8], _written_len: usize) {
        unsafe { ::flatbuffers::emplace_scalar::<u8>(dst, self.0) };
    }
}

impl ::flatbuffers::EndianScalar for PositionCovarianceType {
  type Scalar = u8;
  #[inline]
  fn to_little_endian(self) -> u8 {
    self.0.to_le()
  }
  #[inline]
  #[allow(clippy::wrong_self_convention)]
  fn from_little_endian(v: u8) -> Self {
    let b = u8::from_le(v);
    Self(b)
  }
}

impl<'a> ::flatbuffers::Verifiable for PositionCovarianceType {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    u8::run_verifier(v, pos)
  }
}

impl ::flatbuffers::SimpleToVerifyInSlice for PositionCovarianceType {}
pub enum LocationFixOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A navigation satellite fix for any Global Navigation Satellite System
pub struct LocationFix<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for LocationFix<'a> {
  type Inner = LocationFix<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> LocationFix<'a> {
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
  pub const VT_FRAME_ID: ::flatbuffers::VOffsetT = 6;
  pub const VT_LATITUDE: ::flatbuffers::VOffsetT = 8;
  pub const VT_LONGITUDE: ::flatbuffers::VOffsetT = 10;
  pub const VT_ALTITUDE: ::flatbuffers::VOffsetT = 12;
  pub const VT_POSITION_COVARIANCE: ::flatbuffers::VOffsetT = 14;
  pub const VT_POSITION_COVARIANCE_TYPE: ::flatbuffers::VOffsetT = 16;
  pub const VT_COLOR: ::flatbuffers::VOffsetT = 18;
  pub const VT_METADATA: ::flatbuffers::VOffsetT = 20;
  pub const VT_HEADING: ::flatbuffers::VOffsetT = 22;
  pub const VT_VELOCITY: ::flatbuffers::VOffsetT = 24;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    LocationFix { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args LocationFixArgs<'args>
  ) -> ::flatbuffers::WIPOffset<LocationFix<'bldr>> {
    let mut builder = LocationFixBuilder::new(_fbb);
    if let Some(x) = args.heading { builder.add_heading(x); }
    builder.add_altitude(args.altitude);
    builder.add_longitude(args.longitude);
    builder.add_latitude(args.latitude);
    if let Some(x) = args.velocity { builder.add_velocity(x); }
    if let Some(x) = args.metadata { builder.add_metadata(x); }
    if let Some(x) = args.color { builder.add_color(x); }
    if let Some(x) = args.position_covariance { builder.add_position_covariance(x); }
    if let Some(x) = args.frame_id { builder.add_frame_id(x); }
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    builder.add_position_covariance_type(args.position_covariance_type);
    builder.finish()
  }


  /// Timestamp of the message
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(LocationFix::VT_TIMESTAMP, None)}
  }
  /// Frame for the sensor. Latitude and longitude readings are at the origin of the frame.
  #[inline]
  pub fn frame_id(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(LocationFix::VT_FRAME_ID, None)}
  }
  /// Latitude in degrees
  #[inline]
  pub fn latitude(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(LocationFix::VT_LATITUDE, Some(0.0)).unwrap()}
  }
  /// Longitude in degrees
  #[inline]
  pub fn longitude(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(LocationFix::VT_LONGITUDE, Some(0.0)).unwrap()}
  }
  /// Altitude in meters
  #[inline]
  pub fn altitude(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(LocationFix::VT_ALTITUDE, Some(0.0)).unwrap()}
  }
  /// Position covariance (m^2) defined relative to a tangential plane through the reported position. The components are East, North, and Up (ENU), in row-major order.
  /// length 9
  #[inline]
  pub fn position_covariance(&self) -> Option<::flatbuffers::Vector<'a, f64>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, f64>>>(LocationFix::VT_POSITION_COVARIANCE, None)}
  }
  /// If `position_covariance` is available, `position_covariance_type` must be set to indicate the type of covariance.
  #[inline]
  pub fn position_covariance_type(&self) -> PositionCovarianceType {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<PositionCovarianceType>(LocationFix::VT_POSITION_COVARIANCE_TYPE, Some(PositionCovarianceType::UNKNOWN)).unwrap()}
  }
  /// Color used to visualize the location
  #[inline]
  pub fn color(&self) -> Option<Color<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Color>>(LocationFix::VT_COLOR, None)}
  }
  /// Additional user-provided metadata associated with the location fix. Keys must be unique.
  #[inline]
  pub fn metadata(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<KeyValuePair<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<KeyValuePair>>>>(LocationFix::VT_METADATA, None)}
  }
  /// Heading (yaw angle), in radians, measured clockwise from north
  #[inline]
  pub fn heading(&self) -> Option<f64> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(LocationFix::VT_HEADING, None)}
  }
  /// Velocity in local East-North-Up (ENU) frame in m/s
  #[inline]
  pub fn velocity(&self) -> Option<Vector3<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Vector3>>(LocationFix::VT_VELOCITY, None)}
  }
}

impl ::flatbuffers::Verifiable for LocationFix<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("frame_id", Self::VT_FRAME_ID, false)?
     .visit_field::<f64>("latitude", Self::VT_LATITUDE, false)?
     .visit_field::<f64>("longitude", Self::VT_LONGITUDE, false)?
     .visit_field::<f64>("altitude", Self::VT_ALTITUDE, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, f64>>>("position_covariance", Self::VT_POSITION_COVARIANCE, false)?
     .visit_field::<PositionCovarianceType>("position_covariance_type", Self::VT_POSITION_COVARIANCE_TYPE, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Color>>("color", Self::VT_COLOR, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<KeyValuePair>>>>("metadata", Self::VT_METADATA, false)?
     .visit_field::<f64>("heading", Self::VT_HEADING, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Vector3>>("velocity", Self::VT_VELOCITY, false)?
     .finish();
    Ok(())
  }
}
pub struct LocationFixArgs<'a> {
    pub timestamp: Option<&'a Time>,
    pub frame_id: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: f64,
    pub position_covariance: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, f64>>>,
    pub position_covariance_type: PositionCovarianceType,
    pub color: Option<::flatbuffers::WIPOffset<Color<'a>>>,
    pub metadata: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<KeyValuePair<'a>>>>>,
    pub heading: Option<f64>,
    pub velocity: Option<::flatbuffers::WIPOffset<Vector3<'a>>>,
}
impl<'a> Default for LocationFixArgs<'a> {
  #[inline]
  fn default() -> Self {
    LocationFixArgs {
      timestamp: None,
      frame_id: None,
      latitude: 0.0,
      longitude: 0.0,
      altitude: 0.0,
      position_covariance: None,
      position_covariance_type: PositionCovarianceType::UNKNOWN,
      color: None,
      metadata: None,
      heading: None,
      velocity: None,
    }
  }
}

pub struct LocationFixBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> LocationFixBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(LocationFix::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn add_frame_id(&mut self, frame_id: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(LocationFix::VT_FRAME_ID, frame_id);
  }
  #[inline]
  pub fn add_latitude(&mut self, latitude: f64) {
    self.fbb_.push_slot::<f64>(LocationFix::VT_LATITUDE, latitude, 0.0);
  }
  #[inline]
  pub fn add_longitude(&mut self, longitude: f64) {
    self.fbb_.push_slot::<f64>(LocationFix::VT_LONGITUDE, longitude, 0.0);
  }
  #[inline]
  pub fn add_altitude(&mut self, altitude: f64) {
    self.fbb_.push_slot::<f64>(LocationFix::VT_ALTITUDE, altitude, 0.0);
  }
  #[inline]
  pub fn add_position_covariance(&mut self, position_covariance: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , f64>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(LocationFix::VT_POSITION_COVARIANCE, position_covariance);
  }
  #[inline]
  pub fn add_position_covariance_type(&mut self, position_covariance_type: PositionCovarianceType) {
    self.fbb_.push_slot::<PositionCovarianceType>(LocationFix::VT_POSITION_COVARIANCE_TYPE, position_covariance_type, PositionCovarianceType::UNKNOWN);
  }
  #[inline]
  pub fn add_color(&mut self, color: ::flatbuffers::WIPOffset<Color<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Color>>(LocationFix::VT_COLOR, color);
  }
  #[inline]
  pub fn add_metadata(&mut self, metadata: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<KeyValuePair<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(LocationFix::VT_METADATA, metadata);
  }
  #[inline]
  pub fn add_heading(&mut self, heading: f64) {
    self.fbb_.push_slot_always::<f64>(LocationFix::VT_HEADING, heading);
  }
  #[inline]
  pub fn add_velocity(&mut self, velocity: ::flatbuffers::WIPOffset<Vector3<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Vector3>>(LocationFix::VT_VELOCITY, velocity);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> LocationFixBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    LocationFixBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<LocationFix<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for LocationFix<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("LocationFix");
      ds.field("timestamp", &self.timestamp());
      ds.field("frame_id", &self.frame_id());
      ds.field("latitude", &self.latitude());
      ds.field("longitude", &self.longitude());
      ds.field("altitude", &self.altitude());
      ds.field("position_covariance", &self.position_covariance());
      ds.field("position_covariance_type", &self.position_covariance_type());
      ds.field("color", &self.color());
      ds.field("metadata", &self.metadata());
      ds.field("heading", &self.heading());
      ds.field("velocity", &self.velocity());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `LocationFix`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_location_fix_unchecked`.
pub fn root_as_location_fix(buf: &[u8]) -> Result<LocationFix<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<LocationFix>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `LocationFix` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_location_fix_unchecked`.
pub fn size_prefixed_root_as_location_fix(buf: &[u8]) -> Result<LocationFix<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<LocationFix>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `LocationFix` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_location_fix_unchecked`.
pub fn root_as_location_fix_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<LocationFix<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<LocationFix<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `LocationFix` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_location_fix_unchecked`.
pub fn size_prefixed_root_as_location_fix_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<LocationFix<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<LocationFix<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a LocationFix and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `LocationFix`.
pub unsafe fn root_as_location_fix_unchecked(buf: &[u8]) -> LocationFix<'_> {
  unsafe { ::flatbuffers::root_unchecked::<LocationFix>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed LocationFix and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `LocationFix`.
pub unsafe fn size_prefixed_root_as_location_fix_unchecked(buf: &[u8]) -> LocationFix<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<LocationFix>(buf) }
}
#[inline]
pub fn finish_location_fix_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<LocationFix<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_location_fix_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<LocationFix<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from LocationFixes_generated.rs =====



pub enum LocationFixesOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A group of LocationFix messages
pub struct LocationFixes<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for LocationFixes<'a> {
  type Inner = LocationFixes<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> LocationFixes<'a> {
  pub const VT_FIXES: ::flatbuffers::VOffsetT = 4;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    LocationFixes { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args LocationFixesArgs<'args>
  ) -> ::flatbuffers::WIPOffset<LocationFixes<'bldr>> {
    let mut builder = LocationFixesBuilder::new(_fbb);
    if let Some(x) = args.fixes { builder.add_fixes(x); }
    builder.finish()
  }


  /// An array of location fixes
  #[inline]
  pub fn fixes(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<LocationFix<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<LocationFix>>>>(LocationFixes::VT_FIXES, None)}
  }
}

impl ::flatbuffers::Verifiable for LocationFixes<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<LocationFix>>>>("fixes", Self::VT_FIXES, false)?
     .finish();
    Ok(())
  }
}
pub struct LocationFixesArgs<'a> {
    pub fixes: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<LocationFix<'a>>>>>,
}
impl<'a> Default for LocationFixesArgs<'a> {
  #[inline]
  fn default() -> Self {
    LocationFixesArgs {
      fixes: None,
    }
  }
}

pub struct LocationFixesBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> LocationFixesBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_fixes(&mut self, fixes: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<LocationFix<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(LocationFixes::VT_FIXES, fixes);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> LocationFixesBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    LocationFixesBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<LocationFixes<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for LocationFixes<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("LocationFixes");
      ds.field("fixes", &self.fixes());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `LocationFixes`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_location_fixes_unchecked`.
pub fn root_as_location_fixes(buf: &[u8]) -> Result<LocationFixes<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<LocationFixes>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `LocationFixes` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_location_fixes_unchecked`.
pub fn size_prefixed_root_as_location_fixes(buf: &[u8]) -> Result<LocationFixes<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<LocationFixes>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `LocationFixes` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_location_fixes_unchecked`.
pub fn root_as_location_fixes_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<LocationFixes<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<LocationFixes<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `LocationFixes` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_location_fixes_unchecked`.
pub fn size_prefixed_root_as_location_fixes_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<LocationFixes<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<LocationFixes<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a LocationFixes and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `LocationFixes`.
pub unsafe fn root_as_location_fixes_unchecked(buf: &[u8]) -> LocationFixes<'_> {
  unsafe { ::flatbuffers::root_unchecked::<LocationFixes>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed LocationFixes and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `LocationFixes`.
pub unsafe fn size_prefixed_root_as_location_fixes_unchecked(buf: &[u8]) -> LocationFixes<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<LocationFixes>(buf) }
}
#[inline]
pub fn finish_location_fixes_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<LocationFixes<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_location_fixes_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<LocationFixes<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from Log_generated.rs =====



#[deprecated(since = "2.0.0", note = "Use associated constants instead. This will no longer be generated in 2021.")]
pub const ENUM_MIN_LOG_LEVEL: u8 = 0;
#[deprecated(since = "2.0.0", note = "Use associated constants instead. This will no longer be generated in 2021.")]
pub const ENUM_MAX_LOG_LEVEL: u8 = 5;
#[deprecated(since = "2.0.0", note = "Use associated constants instead. This will no longer be generated in 2021.")]
#[allow(non_camel_case_types)]
pub const ENUM_VALUES_LOG_LEVEL: [LogLevel; 6] = [
  LogLevel::UNKNOWN,
  LogLevel::DEBUG,
  LogLevel::INFO,
  LogLevel::WARNING,
  LogLevel::ERROR,
  LogLevel::FATAL,
];

/// Log level
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct LogLevel(pub u8);
#[allow(non_upper_case_globals)]
impl LogLevel {
  /// Unknown log level
  pub const UNKNOWN: Self = Self(0);
  /// Debug log level
  pub const DEBUG: Self = Self(1);
  /// Info log level
  pub const INFO: Self = Self(2);
  /// Warning log level
  pub const WARNING: Self = Self(3);
  /// Error log level
  pub const ERROR: Self = Self(4);
  /// Fatal log level
  pub const FATAL: Self = Self(5);

  pub const ENUM_MIN: u8 = 0;
  pub const ENUM_MAX: u8 = 5;
  pub const ENUM_VALUES: &'static [Self] = &[
    Self::UNKNOWN,
    Self::DEBUG,
    Self::INFO,
    Self::WARNING,
    Self::ERROR,
    Self::FATAL,
  ];
  /// Returns the variant's name or "" if unknown.
  pub fn variant_name(self) -> Option<&'static str> {
    match self {
      Self::UNKNOWN => Some("UNKNOWN"),
      Self::DEBUG => Some("DEBUG"),
      Self::INFO => Some("INFO"),
      Self::WARNING => Some("WARNING"),
      Self::ERROR => Some("ERROR"),
      Self::FATAL => Some("FATAL"),
      _ => None,
    }
  }
}
impl ::core::fmt::Debug for LogLevel {
  fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
    if let Some(name) = self.variant_name() {
      f.write_str(name)
    } else {
      f.write_fmt(format_args!("<UNKNOWN {:?}>", self.0))
    }
  }
}
impl<'a> ::flatbuffers::Follow<'a> for LogLevel {
  type Inner = Self;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    let b = unsafe { ::flatbuffers::read_scalar_at::<u8>(buf, loc) };
    Self(b)
  }
}

impl ::flatbuffers::Push for LogLevel {
    type Output = LogLevel;
    #[inline]
    unsafe fn push(&self, dst: &mut [u8], _written_len: usize) {
        unsafe { ::flatbuffers::emplace_scalar::<u8>(dst, self.0) };
    }
}

impl ::flatbuffers::EndianScalar for LogLevel {
  type Scalar = u8;
  #[inline]
  fn to_little_endian(self) -> u8 {
    self.0.to_le()
  }
  #[inline]
  #[allow(clippy::wrong_self_convention)]
  fn from_little_endian(v: u8) -> Self {
    let b = u8::from_le(v);
    Self(b)
  }
}

impl<'a> ::flatbuffers::Verifiable for LogLevel {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    u8::run_verifier(v, pos)
  }
}

impl ::flatbuffers::SimpleToVerifyInSlice for LogLevel {}
pub enum LogOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A log message
pub struct Log<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for Log<'a> {
  type Inner = Log<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> Log<'a> {
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
  pub const VT_LEVEL: ::flatbuffers::VOffsetT = 6;
  pub const VT_MESSAGE: ::flatbuffers::VOffsetT = 8;
  pub const VT_NAME: ::flatbuffers::VOffsetT = 10;
  pub const VT_FILE: ::flatbuffers::VOffsetT = 12;
  pub const VT_LINE: ::flatbuffers::VOffsetT = 14;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    Log { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args LogArgs<'args>
  ) -> ::flatbuffers::WIPOffset<Log<'bldr>> {
    let mut builder = LogBuilder::new(_fbb);
    builder.add_line(args.line);
    if let Some(x) = args.file { builder.add_file(x); }
    if let Some(x) = args.name { builder.add_name(x); }
    if let Some(x) = args.message { builder.add_message(x); }
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    builder.add_level(args.level);
    builder.finish()
  }


  /// Timestamp of log message
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(Log::VT_TIMESTAMP, None)}
  }
  /// Log level
  #[inline]
  pub fn level(&self) -> LogLevel {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<LogLevel>(Log::VT_LEVEL, Some(LogLevel::UNKNOWN)).unwrap()}
  }
  /// Log message
  #[inline]
  pub fn message(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(Log::VT_MESSAGE, None)}
  }
  /// Process or node name
  #[inline]
  pub fn name(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(Log::VT_NAME, None)}
  }
  /// Filename
  #[inline]
  pub fn file(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(Log::VT_FILE, None)}
  }
  /// Line number in the file
  #[inline]
  pub fn line(&self) -> u32 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<u32>(Log::VT_LINE, Some(0)).unwrap()}
  }
}

impl ::flatbuffers::Verifiable for Log<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<LogLevel>("level", Self::VT_LEVEL, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("message", Self::VT_MESSAGE, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("name", Self::VT_NAME, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("file", Self::VT_FILE, false)?
     .visit_field::<u32>("line", Self::VT_LINE, false)?
     .finish();
    Ok(())
  }
}
pub struct LogArgs<'a> {
    pub timestamp: Option<&'a Time>,
    pub level: LogLevel,
    pub message: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub name: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub file: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub line: u32,
}
impl<'a> Default for LogArgs<'a> {
  #[inline]
  fn default() -> Self {
    LogArgs {
      timestamp: None,
      level: LogLevel::UNKNOWN,
      message: None,
      name: None,
      file: None,
      line: 0,
    }
  }
}

pub struct LogBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> LogBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(Log::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn add_level(&mut self, level: LogLevel) {
    self.fbb_.push_slot::<LogLevel>(Log::VT_LEVEL, level, LogLevel::UNKNOWN);
  }
  #[inline]
  pub fn add_message(&mut self, message: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(Log::VT_MESSAGE, message);
  }
  #[inline]
  pub fn add_name(&mut self, name: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(Log::VT_NAME, name);
  }
  #[inline]
  pub fn add_file(&mut self, file: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(Log::VT_FILE, file);
  }
  #[inline]
  pub fn add_line(&mut self, line: u32) {
    self.fbb_.push_slot::<u32>(Log::VT_LINE, line, 0);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> LogBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    LogBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<Log<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for Log<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("Log");
      ds.field("timestamp", &self.timestamp());
      ds.field("level", &self.level());
      ds.field("message", &self.message());
      ds.field("name", &self.name());
      ds.field("file", &self.file());
      ds.field("line", &self.line());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `Log`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_log_unchecked`.
pub fn root_as_log(buf: &[u8]) -> Result<Log<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<Log>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `Log` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_log_unchecked`.
pub fn size_prefixed_root_as_log(buf: &[u8]) -> Result<Log<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<Log>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `Log` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_log_unchecked`.
pub fn root_as_log_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<Log<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<Log<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `Log` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_log_unchecked`.
pub fn size_prefixed_root_as_log_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<Log<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<Log<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a Log and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `Log`.
pub unsafe fn root_as_log_unchecked(buf: &[u8]) -> Log<'_> {
  unsafe { ::flatbuffers::root_unchecked::<Log>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed Log and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `Log`.
pub unsafe fn size_prefixed_root_as_log_unchecked(buf: &[u8]) -> Log<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<Log>(buf) }
}
#[inline]
pub fn finish_log_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<Log<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_log_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<Log<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from ModelPrimitive_generated.rs =====



pub enum ModelPrimitiveOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A primitive representing a 3D model file loaded from an external URL or embedded data
pub struct ModelPrimitive<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for ModelPrimitive<'a> {
  type Inner = ModelPrimitive<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> ModelPrimitive<'a> {
  pub const VT_POSE: ::flatbuffers::VOffsetT = 4;
  pub const VT_SCALE: ::flatbuffers::VOffsetT = 6;
  pub const VT_COLOR: ::flatbuffers::VOffsetT = 8;
  pub const VT_OVERRIDE_COLOR: ::flatbuffers::VOffsetT = 10;
  pub const VT_URL: ::flatbuffers::VOffsetT = 12;
  pub const VT_MEDIA_TYPE: ::flatbuffers::VOffsetT = 14;
  pub const VT_DATA: ::flatbuffers::VOffsetT = 16;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    ModelPrimitive { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args ModelPrimitiveArgs<'args>
  ) -> ::flatbuffers::WIPOffset<ModelPrimitive<'bldr>> {
    let mut builder = ModelPrimitiveBuilder::new(_fbb);
    if let Some(x) = args.data { builder.add_data(x); }
    if let Some(x) = args.media_type { builder.add_media_type(x); }
    if let Some(x) = args.url { builder.add_url(x); }
    if let Some(x) = args.color { builder.add_color(x); }
    if let Some(x) = args.scale { builder.add_scale(x); }
    if let Some(x) = args.pose { builder.add_pose(x); }
    builder.add_override_color(args.override_color);
    builder.finish()
  }


  /// Origin of model relative to reference frame
  #[inline]
  pub fn pose(&self) -> Option<Pose<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Pose>>(ModelPrimitive::VT_POSE, None)}
  }
  /// Scale factor to apply to the model along each axis
  #[inline]
  pub fn scale(&self) -> Option<Vector3<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Vector3>>(ModelPrimitive::VT_SCALE, None)}
  }
  /// Solid color to use for the whole model if `override_color` is true.
  #[inline]
  pub fn color(&self) -> Option<Color<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Color>>(ModelPrimitive::VT_COLOR, None)}
  }
  /// Whether to use the color specified in `color` instead of any materials embedded in the original model.
  #[inline]
  pub fn override_color(&self) -> bool {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<bool>(ModelPrimitive::VT_OVERRIDE_COLOR, Some(false)).unwrap()}
  }
  /// URL pointing to model file. One of `url` or `data` should be non-empty.
  #[inline]
  pub fn url(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(ModelPrimitive::VT_URL, None)}
  }
  /// [Media type](https://developer.mozilla.org/en-US/docs/Web/HTTP/Basics_of_HTTP/MIME_types) of embedded model (e.g. `model/gltf-binary`). Required if `data` is provided instead of `url`. Overrides the inferred media type if `url` is provided.
  #[inline]
  pub fn media_type(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(ModelPrimitive::VT_MEDIA_TYPE, None)}
  }
  /// Embedded model. One of `url` or `data` should be non-empty. If `data` is non-empty, `media_type` must be set to indicate the type of the data.
  #[inline]
  pub fn data(&self) -> Option<::flatbuffers::Vector<'a, u8>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, u8>>>(ModelPrimitive::VT_DATA, None)}
  }
}

impl ::flatbuffers::Verifiable for ModelPrimitive<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Pose>>("pose", Self::VT_POSE, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Vector3>>("scale", Self::VT_SCALE, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Color>>("color", Self::VT_COLOR, false)?
     .visit_field::<bool>("override_color", Self::VT_OVERRIDE_COLOR, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("url", Self::VT_URL, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("media_type", Self::VT_MEDIA_TYPE, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, u8>>>("data", Self::VT_DATA, false)?
     .finish();
    Ok(())
  }
}
pub struct ModelPrimitiveArgs<'a> {
    pub pose: Option<::flatbuffers::WIPOffset<Pose<'a>>>,
    pub scale: Option<::flatbuffers::WIPOffset<Vector3<'a>>>,
    pub color: Option<::flatbuffers::WIPOffset<Color<'a>>>,
    pub override_color: bool,
    pub url: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub media_type: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub data: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, u8>>>,
}
impl<'a> Default for ModelPrimitiveArgs<'a> {
  #[inline]
  fn default() -> Self {
    ModelPrimitiveArgs {
      pose: None,
      scale: None,
      color: None,
      override_color: false,
      url: None,
      media_type: None,
      data: None,
    }
  }
}

pub struct ModelPrimitiveBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> ModelPrimitiveBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_pose(&mut self, pose: ::flatbuffers::WIPOffset<Pose<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Pose>>(ModelPrimitive::VT_POSE, pose);
  }
  #[inline]
  pub fn add_scale(&mut self, scale: ::flatbuffers::WIPOffset<Vector3<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Vector3>>(ModelPrimitive::VT_SCALE, scale);
  }
  #[inline]
  pub fn add_color(&mut self, color: ::flatbuffers::WIPOffset<Color<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Color>>(ModelPrimitive::VT_COLOR, color);
  }
  #[inline]
  pub fn add_override_color(&mut self, override_color: bool) {
    self.fbb_.push_slot::<bool>(ModelPrimitive::VT_OVERRIDE_COLOR, override_color, false);
  }
  #[inline]
  pub fn add_url(&mut self, url: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(ModelPrimitive::VT_URL, url);
  }
  #[inline]
  pub fn add_media_type(&mut self, media_type: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(ModelPrimitive::VT_MEDIA_TYPE, media_type);
  }
  #[inline]
  pub fn add_data(&mut self, data: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , u8>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(ModelPrimitive::VT_DATA, data);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> ModelPrimitiveBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    ModelPrimitiveBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<ModelPrimitive<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for ModelPrimitive<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("ModelPrimitive");
      ds.field("pose", &self.pose());
      ds.field("scale", &self.scale());
      ds.field("color", &self.color());
      ds.field("override_color", &self.override_color());
      ds.field("url", &self.url());
      ds.field("media_type", &self.media_type());
      ds.field("data", &self.data());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `ModelPrimitive`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_model_primitive_unchecked`.
pub fn root_as_model_primitive(buf: &[u8]) -> Result<ModelPrimitive<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<ModelPrimitive>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `ModelPrimitive` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_model_primitive_unchecked`.
pub fn size_prefixed_root_as_model_primitive(buf: &[u8]) -> Result<ModelPrimitive<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<ModelPrimitive>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `ModelPrimitive` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_model_primitive_unchecked`.
pub fn root_as_model_primitive_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<ModelPrimitive<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<ModelPrimitive<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `ModelPrimitive` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_model_primitive_unchecked`.
pub fn size_prefixed_root_as_model_primitive_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<ModelPrimitive<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<ModelPrimitive<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a ModelPrimitive and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `ModelPrimitive`.
pub unsafe fn root_as_model_primitive_unchecked(buf: &[u8]) -> ModelPrimitive<'_> {
  unsafe { ::flatbuffers::root_unchecked::<ModelPrimitive>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed ModelPrimitive and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `ModelPrimitive`.
pub unsafe fn size_prefixed_root_as_model_primitive_unchecked(buf: &[u8]) -> ModelPrimitive<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<ModelPrimitive>(buf) }
}
#[inline]
pub fn finish_model_primitive_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<ModelPrimitive<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_model_primitive_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<ModelPrimitive<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from Odometry_generated.rs =====



pub enum OdometryOffset {}
#[derive(Copy, Clone, PartialEq)]

/// An estimate of position, orientation, and velocity for an object or reference frame in 3D space
pub struct Odometry<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for Odometry<'a> {
  type Inner = Odometry<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> Odometry<'a> {
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
  pub const VT_FRAME_ID: ::flatbuffers::VOffsetT = 6;
  pub const VT_BODY_FRAME_ID: ::flatbuffers::VOffsetT = 8;
  pub const VT_POSE: ::flatbuffers::VOffsetT = 10;
  pub const VT_LINEAR_VELOCITY: ::flatbuffers::VOffsetT = 12;
  pub const VT_ANGULAR_VELOCITY: ::flatbuffers::VOffsetT = 14;
  pub const VT_POSE_COVARIANCE: ::flatbuffers::VOffsetT = 16;
  pub const VT_VELOCITY_COVARIANCE: ::flatbuffers::VOffsetT = 18;
  pub const VT_METADATA: ::flatbuffers::VOffsetT = 20;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    Odometry { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args OdometryArgs<'args>
  ) -> ::flatbuffers::WIPOffset<Odometry<'bldr>> {
    let mut builder = OdometryBuilder::new(_fbb);
    if let Some(x) = args.metadata { builder.add_metadata(x); }
    if let Some(x) = args.velocity_covariance { builder.add_velocity_covariance(x); }
    if let Some(x) = args.pose_covariance { builder.add_pose_covariance(x); }
    if let Some(x) = args.angular_velocity { builder.add_angular_velocity(x); }
    if let Some(x) = args.linear_velocity { builder.add_linear_velocity(x); }
    if let Some(x) = args.pose { builder.add_pose(x); }
    if let Some(x) = args.body_frame_id { builder.add_body_frame_id(x); }
    if let Some(x) = args.frame_id { builder.add_frame_id(x); }
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    builder.finish()
  }


  /// Timestamp of the message
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(Odometry::VT_TIMESTAMP, None)}
  }
  /// Reference coordinate frame (e.g. `map` or `odom`)
  #[inline]
  pub fn frame_id(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(Odometry::VT_FRAME_ID, None)}
  }
  /// Coordinate frame of the body whose motion is being estimated (e.g. `base_link`)
  #[inline]
  pub fn body_frame_id(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(Odometry::VT_BODY_FRAME_ID, None)}
  }
  /// Position and orientation of body_frame_id in frame_id
  #[inline]
  pub fn pose(&self) -> Option<Pose<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Pose>>(Odometry::VT_POSE, None)}
  }
  /// Linear velocity in m/s in body_frame_id
  #[inline]
  pub fn linear_velocity(&self) -> Option<Vector3<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Vector3>>(Odometry::VT_LINEAR_VELOCITY, None)}
  }
  /// Angular velocity in rad/s in body_frame_id
  #[inline]
  pub fn angular_velocity(&self) -> Option<Vector3<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Vector3>>(Odometry::VT_ANGULAR_VELOCITY, None)}
  }
  /// Row-major 6x6 covariance matrix (x, y, z, rotation about x, rotation about y, rotation about z). Set to zero if unknown.
  /// length 36
  #[inline]
  pub fn pose_covariance(&self) -> Option<::flatbuffers::Vector<'a, f64>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, f64>>>(Odometry::VT_POSE_COVARIANCE, None)}
  }
  /// Row-major 6x6 covariance matrix (vx, vy, vz, angular rate about x, angular rate about y, angular rate about z). Set to zero if unknown.
  /// length 36
  #[inline]
  pub fn velocity_covariance(&self) -> Option<::flatbuffers::Vector<'a, f64>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, f64>>>(Odometry::VT_VELOCITY_COVARIANCE, None)}
  }
  /// Additional user-provided metadata associated with the odometry message. Keys must be unique.
  #[inline]
  pub fn metadata(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<KeyValuePair<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<KeyValuePair>>>>(Odometry::VT_METADATA, None)}
  }
}

impl ::flatbuffers::Verifiable for Odometry<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("frame_id", Self::VT_FRAME_ID, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("body_frame_id", Self::VT_BODY_FRAME_ID, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Pose>>("pose", Self::VT_POSE, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Vector3>>("linear_velocity", Self::VT_LINEAR_VELOCITY, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Vector3>>("angular_velocity", Self::VT_ANGULAR_VELOCITY, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, f64>>>("pose_covariance", Self::VT_POSE_COVARIANCE, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, f64>>>("velocity_covariance", Self::VT_VELOCITY_COVARIANCE, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<KeyValuePair>>>>("metadata", Self::VT_METADATA, false)?
     .finish();
    Ok(())
  }
}
pub struct OdometryArgs<'a> {
    pub timestamp: Option<&'a Time>,
    pub frame_id: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub body_frame_id: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub pose: Option<::flatbuffers::WIPOffset<Pose<'a>>>,
    pub linear_velocity: Option<::flatbuffers::WIPOffset<Vector3<'a>>>,
    pub angular_velocity: Option<::flatbuffers::WIPOffset<Vector3<'a>>>,
    pub pose_covariance: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, f64>>>,
    pub velocity_covariance: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, f64>>>,
    pub metadata: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<KeyValuePair<'a>>>>>,
}
impl<'a> Default for OdometryArgs<'a> {
  #[inline]
  fn default() -> Self {
    OdometryArgs {
      timestamp: None,
      frame_id: None,
      body_frame_id: None,
      pose: None,
      linear_velocity: None,
      angular_velocity: None,
      pose_covariance: None,
      velocity_covariance: None,
      metadata: None,
    }
  }
}

pub struct OdometryBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> OdometryBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(Odometry::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn add_frame_id(&mut self, frame_id: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(Odometry::VT_FRAME_ID, frame_id);
  }
  #[inline]
  pub fn add_body_frame_id(&mut self, body_frame_id: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(Odometry::VT_BODY_FRAME_ID, body_frame_id);
  }
  #[inline]
  pub fn add_pose(&mut self, pose: ::flatbuffers::WIPOffset<Pose<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Pose>>(Odometry::VT_POSE, pose);
  }
  #[inline]
  pub fn add_linear_velocity(&mut self, linear_velocity: ::flatbuffers::WIPOffset<Vector3<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Vector3>>(Odometry::VT_LINEAR_VELOCITY, linear_velocity);
  }
  #[inline]
  pub fn add_angular_velocity(&mut self, angular_velocity: ::flatbuffers::WIPOffset<Vector3<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Vector3>>(Odometry::VT_ANGULAR_VELOCITY, angular_velocity);
  }
  #[inline]
  pub fn add_pose_covariance(&mut self, pose_covariance: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , f64>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(Odometry::VT_POSE_COVARIANCE, pose_covariance);
  }
  #[inline]
  pub fn add_velocity_covariance(&mut self, velocity_covariance: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , f64>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(Odometry::VT_VELOCITY_COVARIANCE, velocity_covariance);
  }
  #[inline]
  pub fn add_metadata(&mut self, metadata: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<KeyValuePair<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(Odometry::VT_METADATA, metadata);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> OdometryBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    OdometryBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<Odometry<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for Odometry<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("Odometry");
      ds.field("timestamp", &self.timestamp());
      ds.field("frame_id", &self.frame_id());
      ds.field("body_frame_id", &self.body_frame_id());
      ds.field("pose", &self.pose());
      ds.field("linear_velocity", &self.linear_velocity());
      ds.field("angular_velocity", &self.angular_velocity());
      ds.field("pose_covariance", &self.pose_covariance());
      ds.field("velocity_covariance", &self.velocity_covariance());
      ds.field("metadata", &self.metadata());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `Odometry`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_odometry_unchecked`.
pub fn root_as_odometry(buf: &[u8]) -> Result<Odometry<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<Odometry>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `Odometry` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_odometry_unchecked`.
pub fn size_prefixed_root_as_odometry(buf: &[u8]) -> Result<Odometry<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<Odometry>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `Odometry` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_odometry_unchecked`.
pub fn root_as_odometry_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<Odometry<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<Odometry<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `Odometry` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_odometry_unchecked`.
pub fn size_prefixed_root_as_odometry_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<Odometry<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<Odometry<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a Odometry and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `Odometry`.
pub unsafe fn root_as_odometry_unchecked(buf: &[u8]) -> Odometry<'_> {
  unsafe { ::flatbuffers::root_unchecked::<Odometry>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed Odometry and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `Odometry`.
pub unsafe fn size_prefixed_root_as_odometry_unchecked(buf: &[u8]) -> Odometry<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<Odometry>(buf) }
}
#[inline]
pub fn finish_odometry_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<Odometry<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_odometry_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<Odometry<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from PackedElementField_generated.rs =====



#[deprecated(since = "2.0.0", note = "Use associated constants instead. This will no longer be generated in 2021.")]
pub const ENUM_MIN_NUMERIC_TYPE: u8 = 0;
#[deprecated(since = "2.0.0", note = "Use associated constants instead. This will no longer be generated in 2021.")]
pub const ENUM_MAX_NUMERIC_TYPE: u8 = 8;
#[deprecated(since = "2.0.0", note = "Use associated constants instead. This will no longer be generated in 2021.")]
#[allow(non_camel_case_types)]
pub const ENUM_VALUES_NUMERIC_TYPE: [NumericType; 9] = [
  NumericType::UNKNOWN,
  NumericType::UINT8,
  NumericType::INT8,
  NumericType::UINT16,
  NumericType::INT16,
  NumericType::UINT32,
  NumericType::INT32,
  NumericType::FLOAT32,
  NumericType::FLOAT64,
];

/// Numeric type
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct NumericType(pub u8);
#[allow(non_upper_case_globals)]
impl NumericType {
  /// Unknown numeric type
  pub const UNKNOWN: Self = Self(0);
  /// Unsigned 8-bit integer
  pub const UINT8: Self = Self(1);
  /// Signed 8-bit integer
  pub const INT8: Self = Self(2);
  /// Unsigned 16-bit integer
  pub const UINT16: Self = Self(3);
  /// Signed 16-bit integer
  pub const INT16: Self = Self(4);
  /// Unsigned 32-bit integer
  pub const UINT32: Self = Self(5);
  /// Signed 32-bit integer
  pub const INT32: Self = Self(6);
  /// 32-bit floating-point number
  pub const FLOAT32: Self = Self(7);
  /// 64-bit floating-point number
  pub const FLOAT64: Self = Self(8);

  pub const ENUM_MIN: u8 = 0;
  pub const ENUM_MAX: u8 = 8;
  pub const ENUM_VALUES: &'static [Self] = &[
    Self::UNKNOWN,
    Self::UINT8,
    Self::INT8,
    Self::UINT16,
    Self::INT16,
    Self::UINT32,
    Self::INT32,
    Self::FLOAT32,
    Self::FLOAT64,
  ];
  /// Returns the variant's name or "" if unknown.
  pub fn variant_name(self) -> Option<&'static str> {
    match self {
      Self::UNKNOWN => Some("UNKNOWN"),
      Self::UINT8 => Some("UINT8"),
      Self::INT8 => Some("INT8"),
      Self::UINT16 => Some("UINT16"),
      Self::INT16 => Some("INT16"),
      Self::UINT32 => Some("UINT32"),
      Self::INT32 => Some("INT32"),
      Self::FLOAT32 => Some("FLOAT32"),
      Self::FLOAT64 => Some("FLOAT64"),
      _ => None,
    }
  }
}
impl ::core::fmt::Debug for NumericType {
  fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
    if let Some(name) = self.variant_name() {
      f.write_str(name)
    } else {
      f.write_fmt(format_args!("<UNKNOWN {:?}>", self.0))
    }
  }
}
impl<'a> ::flatbuffers::Follow<'a> for NumericType {
  type Inner = Self;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    let b = unsafe { ::flatbuffers::read_scalar_at::<u8>(buf, loc) };
    Self(b)
  }
}

impl ::flatbuffers::Push for NumericType {
    type Output = NumericType;
    #[inline]
    unsafe fn push(&self, dst: &mut [u8], _written_len: usize) {
        unsafe { ::flatbuffers::emplace_scalar::<u8>(dst, self.0) };
    }
}

impl ::flatbuffers::EndianScalar for NumericType {
  type Scalar = u8;
  #[inline]
  fn to_little_endian(self) -> u8 {
    self.0.to_le()
  }
  #[inline]
  #[allow(clippy::wrong_self_convention)]
  fn from_little_endian(v: u8) -> Self {
    let b = u8::from_le(v);
    Self(b)
  }
}

impl<'a> ::flatbuffers::Verifiable for NumericType {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    u8::run_verifier(v, pos)
  }
}

impl ::flatbuffers::SimpleToVerifyInSlice for NumericType {}
pub enum PackedElementFieldOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A field present within each element in a byte array of packed elements.
pub struct PackedElementField<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for PackedElementField<'a> {
  type Inner = PackedElementField<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> PackedElementField<'a> {
  pub const VT_NAME: ::flatbuffers::VOffsetT = 4;
  pub const VT_OFFSET: ::flatbuffers::VOffsetT = 6;
  pub const VT_TYPE_: ::flatbuffers::VOffsetT = 8;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    PackedElementField { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args PackedElementFieldArgs<'args>
  ) -> ::flatbuffers::WIPOffset<PackedElementField<'bldr>> {
    let mut builder = PackedElementFieldBuilder::new(_fbb);
    builder.add_offset(args.offset);
    if let Some(x) = args.name { builder.add_name(x); }
    builder.add_type_(args.type_);
    builder.finish()
  }


  /// Name of the field
  #[inline]
  pub fn name(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(PackedElementField::VT_NAME, None)}
  }
  /// Byte offset from start of data buffer
  #[inline]
  pub fn offset(&self) -> u32 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<u32>(PackedElementField::VT_OFFSET, Some(0)).unwrap()}
  }
  /// Type of data in the field. Integers are stored using little-endian byte order.
  #[inline]
  pub fn type_(&self) -> NumericType {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<NumericType>(PackedElementField::VT_TYPE_, Some(NumericType::UNKNOWN)).unwrap()}
  }
}

impl ::flatbuffers::Verifiable for PackedElementField<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("name", Self::VT_NAME, false)?
     .visit_field::<u32>("offset", Self::VT_OFFSET, false)?
     .visit_field::<NumericType>("type_", Self::VT_TYPE_, false)?
     .finish();
    Ok(())
  }
}
pub struct PackedElementFieldArgs<'a> {
    pub name: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub offset: u32,
    pub type_: NumericType,
}
impl<'a> Default for PackedElementFieldArgs<'a> {
  #[inline]
  fn default() -> Self {
    PackedElementFieldArgs {
      name: None,
      offset: 0,
      type_: NumericType::UNKNOWN,
    }
  }
}

pub struct PackedElementFieldBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> PackedElementFieldBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_name(&mut self, name: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(PackedElementField::VT_NAME, name);
  }
  #[inline]
  pub fn add_offset(&mut self, offset: u32) {
    self.fbb_.push_slot::<u32>(PackedElementField::VT_OFFSET, offset, 0);
  }
  #[inline]
  pub fn add_type_(&mut self, type_: NumericType) {
    self.fbb_.push_slot::<NumericType>(PackedElementField::VT_TYPE_, type_, NumericType::UNKNOWN);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> PackedElementFieldBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    PackedElementFieldBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<PackedElementField<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for PackedElementField<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("PackedElementField");
      ds.field("name", &self.name());
      ds.field("offset", &self.offset());
      ds.field("type_", &self.type_());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `PackedElementField`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_packed_element_field_unchecked`.
pub fn root_as_packed_element_field(buf: &[u8]) -> Result<PackedElementField<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<PackedElementField>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `PackedElementField` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_packed_element_field_unchecked`.
pub fn size_prefixed_root_as_packed_element_field(buf: &[u8]) -> Result<PackedElementField<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<PackedElementField>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `PackedElementField` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_packed_element_field_unchecked`.
pub fn root_as_packed_element_field_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<PackedElementField<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<PackedElementField<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `PackedElementField` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_packed_element_field_unchecked`.
pub fn size_prefixed_root_as_packed_element_field_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<PackedElementField<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<PackedElementField<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a PackedElementField and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `PackedElementField`.
pub unsafe fn root_as_packed_element_field_unchecked(buf: &[u8]) -> PackedElementField<'_> {
  unsafe { ::flatbuffers::root_unchecked::<PackedElementField>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed PackedElementField and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `PackedElementField`.
pub unsafe fn size_prefixed_root_as_packed_element_field_unchecked(buf: &[u8]) -> PackedElementField<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<PackedElementField>(buf) }
}
#[inline]
pub fn finish_packed_element_field_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<PackedElementField<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_packed_element_field_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<PackedElementField<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from Point2_generated.rs =====



pub enum Point2Offset {}
#[derive(Copy, Clone, PartialEq)]

/// A point representing a position in 2D space
pub struct Point2<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for Point2<'a> {
  type Inner = Point2<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> Point2<'a> {
  pub const VT_X: ::flatbuffers::VOffsetT = 4;
  pub const VT_Y: ::flatbuffers::VOffsetT = 6;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    Point2 { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args Point2Args
  ) -> ::flatbuffers::WIPOffset<Point2<'bldr>> {
    let mut builder = Point2Builder::new(_fbb);
    builder.add_y(args.y);
    builder.add_x(args.x);
    builder.finish()
  }


  /// x coordinate position
  #[inline]
  pub fn x(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(Point2::VT_X, Some(0.0)).unwrap()}
  }
  /// y coordinate position
  #[inline]
  pub fn y(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(Point2::VT_Y, Some(0.0)).unwrap()}
  }
}

impl ::flatbuffers::Verifiable for Point2<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<f64>("x", Self::VT_X, false)?
     .visit_field::<f64>("y", Self::VT_Y, false)?
     .finish();
    Ok(())
  }
}
pub struct Point2Args {
    pub x: f64,
    pub y: f64,
}
impl<'a> Default for Point2Args {
  #[inline]
  fn default() -> Self {
    Point2Args {
      x: 0.0,
      y: 0.0,
    }
  }
}

pub struct Point2Builder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> Point2Builder<'a, 'b, A> {
  #[inline]
  pub fn add_x(&mut self, x: f64) {
    self.fbb_.push_slot::<f64>(Point2::VT_X, x, 0.0);
  }
  #[inline]
  pub fn add_y(&mut self, y: f64) {
    self.fbb_.push_slot::<f64>(Point2::VT_Y, y, 0.0);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> Point2Builder<'a, 'b, A> {
    let start = _fbb.start_table();
    Point2Builder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<Point2<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for Point2<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("Point2");
      ds.field("x", &self.x());
      ds.field("y", &self.y());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `Point2`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_point_2_unchecked`.
pub fn root_as_point_2(buf: &[u8]) -> Result<Point2<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<Point2>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `Point2` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_point_2_unchecked`.
pub fn size_prefixed_root_as_point_2(buf: &[u8]) -> Result<Point2<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<Point2>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `Point2` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_point_2_unchecked`.
pub fn root_as_point_2_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<Point2<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<Point2<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `Point2` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_point_2_unchecked`.
pub fn size_prefixed_root_as_point_2_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<Point2<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<Point2<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a Point2 and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `Point2`.
pub unsafe fn root_as_point_2_unchecked(buf: &[u8]) -> Point2<'_> {
  unsafe { ::flatbuffers::root_unchecked::<Point2>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed Point2 and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `Point2`.
pub unsafe fn size_prefixed_root_as_point_2_unchecked(buf: &[u8]) -> Point2<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<Point2>(buf) }
}
#[inline]
pub fn finish_point_2_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<Point2<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_point_2_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<Point2<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from Point3InFrame_generated.rs =====



pub enum Point3InFrameOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A timestamped point for a position in 3D space
pub struct Point3InFrame<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for Point3InFrame<'a> {
  type Inner = Point3InFrame<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> Point3InFrame<'a> {
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
  pub const VT_FRAME_ID: ::flatbuffers::VOffsetT = 6;
  pub const VT_POINT: ::flatbuffers::VOffsetT = 8;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    Point3InFrame { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args Point3InFrameArgs<'args>
  ) -> ::flatbuffers::WIPOffset<Point3InFrame<'bldr>> {
    let mut builder = Point3InFrameBuilder::new(_fbb);
    if let Some(x) = args.point { builder.add_point(x); }
    if let Some(x) = args.frame_id { builder.add_frame_id(x); }
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    builder.finish()
  }


  /// Timestamp of point
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(Point3InFrame::VT_TIMESTAMP, None)}
  }
  /// Frame of reference for point position
  #[inline]
  pub fn frame_id(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(Point3InFrame::VT_FRAME_ID, None)}
  }
  /// Point in 3D space
  #[inline]
  pub fn point(&self) -> Option<Point3<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Point3>>(Point3InFrame::VT_POINT, None)}
  }
}

impl ::flatbuffers::Verifiable for Point3InFrame<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("frame_id", Self::VT_FRAME_ID, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Point3>>("point", Self::VT_POINT, false)?
     .finish();
    Ok(())
  }
}
pub struct Point3InFrameArgs<'a> {
    pub timestamp: Option<&'a Time>,
    pub frame_id: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub point: Option<::flatbuffers::WIPOffset<Point3<'a>>>,
}
impl<'a> Default for Point3InFrameArgs<'a> {
  #[inline]
  fn default() -> Self {
    Point3InFrameArgs {
      timestamp: None,
      frame_id: None,
      point: None,
    }
  }
}

pub struct Point3InFrameBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> Point3InFrameBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(Point3InFrame::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn add_frame_id(&mut self, frame_id: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(Point3InFrame::VT_FRAME_ID, frame_id);
  }
  #[inline]
  pub fn add_point(&mut self, point: ::flatbuffers::WIPOffset<Point3<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Point3>>(Point3InFrame::VT_POINT, point);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> Point3InFrameBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    Point3InFrameBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<Point3InFrame<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for Point3InFrame<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("Point3InFrame");
      ds.field("timestamp", &self.timestamp());
      ds.field("frame_id", &self.frame_id());
      ds.field("point", &self.point());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `Point3InFrame`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_point_3_in_frame_unchecked`.
pub fn root_as_point_3_in_frame(buf: &[u8]) -> Result<Point3InFrame<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<Point3InFrame>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `Point3InFrame` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_point_3_in_frame_unchecked`.
pub fn size_prefixed_root_as_point_3_in_frame(buf: &[u8]) -> Result<Point3InFrame<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<Point3InFrame>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `Point3InFrame` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_point_3_in_frame_unchecked`.
pub fn root_as_point_3_in_frame_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<Point3InFrame<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<Point3InFrame<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `Point3InFrame` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_point_3_in_frame_unchecked`.
pub fn size_prefixed_root_as_point_3_in_frame_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<Point3InFrame<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<Point3InFrame<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a Point3InFrame and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `Point3InFrame`.
pub unsafe fn root_as_point_3_in_frame_unchecked(buf: &[u8]) -> Point3InFrame<'_> {
  unsafe { ::flatbuffers::root_unchecked::<Point3InFrame>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed Point3InFrame and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `Point3InFrame`.
pub unsafe fn size_prefixed_root_as_point_3_in_frame_unchecked(buf: &[u8]) -> Point3InFrame<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<Point3InFrame>(buf) }
}
#[inline]
pub fn finish_point_3_in_frame_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<Point3InFrame<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_point_3_in_frame_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<Point3InFrame<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from Point3_generated.rs =====



pub enum Point3Offset {}
#[derive(Copy, Clone, PartialEq)]

/// A point representing a position in 3D space
pub struct Point3<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for Point3<'a> {
  type Inner = Point3<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> Point3<'a> {
  pub const VT_X: ::flatbuffers::VOffsetT = 4;
  pub const VT_Y: ::flatbuffers::VOffsetT = 6;
  pub const VT_Z: ::flatbuffers::VOffsetT = 8;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    Point3 { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args Point3Args
  ) -> ::flatbuffers::WIPOffset<Point3<'bldr>> {
    let mut builder = Point3Builder::new(_fbb);
    builder.add_z(args.z);
    builder.add_y(args.y);
    builder.add_x(args.x);
    builder.finish()
  }


  /// x coordinate position
  #[inline]
  pub fn x(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(Point3::VT_X, Some(0.0)).unwrap()}
  }
  /// y coordinate position
  #[inline]
  pub fn y(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(Point3::VT_Y, Some(0.0)).unwrap()}
  }
  /// z coordinate position
  #[inline]
  pub fn z(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(Point3::VT_Z, Some(0.0)).unwrap()}
  }
}

impl ::flatbuffers::Verifiable for Point3<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<f64>("x", Self::VT_X, false)?
     .visit_field::<f64>("y", Self::VT_Y, false)?
     .visit_field::<f64>("z", Self::VT_Z, false)?
     .finish();
    Ok(())
  }
}
pub struct Point3Args {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}
impl<'a> Default for Point3Args {
  #[inline]
  fn default() -> Self {
    Point3Args {
      x: 0.0,
      y: 0.0,
      z: 0.0,
    }
  }
}

pub struct Point3Builder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> Point3Builder<'a, 'b, A> {
  #[inline]
  pub fn add_x(&mut self, x: f64) {
    self.fbb_.push_slot::<f64>(Point3::VT_X, x, 0.0);
  }
  #[inline]
  pub fn add_y(&mut self, y: f64) {
    self.fbb_.push_slot::<f64>(Point3::VT_Y, y, 0.0);
  }
  #[inline]
  pub fn add_z(&mut self, z: f64) {
    self.fbb_.push_slot::<f64>(Point3::VT_Z, z, 0.0);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> Point3Builder<'a, 'b, A> {
    let start = _fbb.start_table();
    Point3Builder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<Point3<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for Point3<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("Point3");
      ds.field("x", &self.x());
      ds.field("y", &self.y());
      ds.field("z", &self.z());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `Point3`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_point_3_unchecked`.
pub fn root_as_point_3(buf: &[u8]) -> Result<Point3<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<Point3>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `Point3` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_point_3_unchecked`.
pub fn size_prefixed_root_as_point_3(buf: &[u8]) -> Result<Point3<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<Point3>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `Point3` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_point_3_unchecked`.
pub fn root_as_point_3_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<Point3<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<Point3<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `Point3` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_point_3_unchecked`.
pub fn size_prefixed_root_as_point_3_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<Point3<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<Point3<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a Point3 and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `Point3`.
pub unsafe fn root_as_point_3_unchecked(buf: &[u8]) -> Point3<'_> {
  unsafe { ::flatbuffers::root_unchecked::<Point3>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed Point3 and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `Point3`.
pub unsafe fn size_prefixed_root_as_point_3_unchecked(buf: &[u8]) -> Point3<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<Point3>(buf) }
}
#[inline]
pub fn finish_point_3_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<Point3<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_point_3_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<Point3<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from PointCloud_generated.rs =====



pub enum PointCloudOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A collection of N-dimensional points, which may contain additional fields with information like normals, intensity, etc.
pub struct PointCloud<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for PointCloud<'a> {
  type Inner = PointCloud<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> PointCloud<'a> {
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
  pub const VT_FRAME_ID: ::flatbuffers::VOffsetT = 6;
  pub const VT_POSE: ::flatbuffers::VOffsetT = 8;
  pub const VT_POINT_STRIDE: ::flatbuffers::VOffsetT = 10;
  pub const VT_FIELDS: ::flatbuffers::VOffsetT = 12;
  pub const VT_DATA: ::flatbuffers::VOffsetT = 14;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    PointCloud { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args PointCloudArgs<'args>
  ) -> ::flatbuffers::WIPOffset<PointCloud<'bldr>> {
    let mut builder = PointCloudBuilder::new(_fbb);
    if let Some(x) = args.data { builder.add_data(x); }
    if let Some(x) = args.fields { builder.add_fields(x); }
    builder.add_point_stride(args.point_stride);
    if let Some(x) = args.pose { builder.add_pose(x); }
    if let Some(x) = args.frame_id { builder.add_frame_id(x); }
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    builder.finish()
  }


  /// Timestamp of point cloud
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(PointCloud::VT_TIMESTAMP, None)}
  }
  /// Frame of reference
  #[inline]
  pub fn frame_id(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(PointCloud::VT_FRAME_ID, None)}
  }
  /// The origin of the point cloud relative to the frame of reference
  #[inline]
  pub fn pose(&self) -> Option<Pose<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Pose>>(PointCloud::VT_POSE, None)}
  }
  /// Number of bytes between points in the `data`
  #[inline]
  pub fn point_stride(&self) -> u32 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<u32>(PointCloud::VT_POINT_STRIDE, Some(0)).unwrap()}
  }
  /// Fields in `data`. At least 2 coordinate fields from `x`, `y`, and `z` are required for each point's position; `red`, `green`, `blue`, and `alpha` are optional for customizing each point's color.
  #[inline]
  pub fn fields(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<PackedElementField<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<PackedElementField>>>>(PointCloud::VT_FIELDS, None)}
  }
  /// Point data, interpreted using `fields`
  #[inline]
  pub fn data(&self) -> Option<::flatbuffers::Vector<'a, u8>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, u8>>>(PointCloud::VT_DATA, None)}
  }
}

impl ::flatbuffers::Verifiable for PointCloud<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("frame_id", Self::VT_FRAME_ID, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Pose>>("pose", Self::VT_POSE, false)?
     .visit_field::<u32>("point_stride", Self::VT_POINT_STRIDE, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<PackedElementField>>>>("fields", Self::VT_FIELDS, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, u8>>>("data", Self::VT_DATA, false)?
     .finish();
    Ok(())
  }
}
pub struct PointCloudArgs<'a> {
    pub timestamp: Option<&'a Time>,
    pub frame_id: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub pose: Option<::flatbuffers::WIPOffset<Pose<'a>>>,
    pub point_stride: u32,
    pub fields: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<PackedElementField<'a>>>>>,
    pub data: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, u8>>>,
}
impl<'a> Default for PointCloudArgs<'a> {
  #[inline]
  fn default() -> Self {
    PointCloudArgs {
      timestamp: None,
      frame_id: None,
      pose: None,
      point_stride: 0,
      fields: None,
      data: None,
    }
  }
}

pub struct PointCloudBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> PointCloudBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(PointCloud::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn add_frame_id(&mut self, frame_id: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(PointCloud::VT_FRAME_ID, frame_id);
  }
  #[inline]
  pub fn add_pose(&mut self, pose: ::flatbuffers::WIPOffset<Pose<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Pose>>(PointCloud::VT_POSE, pose);
  }
  #[inline]
  pub fn add_point_stride(&mut self, point_stride: u32) {
    self.fbb_.push_slot::<u32>(PointCloud::VT_POINT_STRIDE, point_stride, 0);
  }
  #[inline]
  pub fn add_fields(&mut self, fields: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<PackedElementField<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(PointCloud::VT_FIELDS, fields);
  }
  #[inline]
  pub fn add_data(&mut self, data: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , u8>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(PointCloud::VT_DATA, data);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> PointCloudBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    PointCloudBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<PointCloud<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for PointCloud<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("PointCloud");
      ds.field("timestamp", &self.timestamp());
      ds.field("frame_id", &self.frame_id());
      ds.field("pose", &self.pose());
      ds.field("point_stride", &self.point_stride());
      ds.field("fields", &self.fields());
      ds.field("data", &self.data());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `PointCloud`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_point_cloud_unchecked`.
pub fn root_as_point_cloud(buf: &[u8]) -> Result<PointCloud<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<PointCloud>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `PointCloud` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_point_cloud_unchecked`.
pub fn size_prefixed_root_as_point_cloud(buf: &[u8]) -> Result<PointCloud<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<PointCloud>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `PointCloud` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_point_cloud_unchecked`.
pub fn root_as_point_cloud_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<PointCloud<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<PointCloud<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `PointCloud` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_point_cloud_unchecked`.
pub fn size_prefixed_root_as_point_cloud_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<PointCloud<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<PointCloud<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a PointCloud and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `PointCloud`.
pub unsafe fn root_as_point_cloud_unchecked(buf: &[u8]) -> PointCloud<'_> {
  unsafe { ::flatbuffers::root_unchecked::<PointCloud>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed PointCloud and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `PointCloud`.
pub unsafe fn size_prefixed_root_as_point_cloud_unchecked(buf: &[u8]) -> PointCloud<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<PointCloud>(buf) }
}
#[inline]
pub fn finish_point_cloud_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<PointCloud<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_point_cloud_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<PointCloud<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from PointsAnnotation_generated.rs =====



#[deprecated(since = "2.0.0", note = "Use associated constants instead. This will no longer be generated in 2021.")]
pub const ENUM_MIN_POINTS_ANNOTATION_TYPE: u8 = 0;
#[deprecated(since = "2.0.0", note = "Use associated constants instead. This will no longer be generated in 2021.")]
pub const ENUM_MAX_POINTS_ANNOTATION_TYPE: u8 = 4;
#[deprecated(since = "2.0.0", note = "Use associated constants instead. This will no longer be generated in 2021.")]
#[allow(non_camel_case_types)]
pub const ENUM_VALUES_POINTS_ANNOTATION_TYPE: [PointsAnnotationType; 5] = [
  PointsAnnotationType::UNKNOWN,
  PointsAnnotationType::POINTS,
  PointsAnnotationType::LINE_LOOP,
  PointsAnnotationType::LINE_STRIP,
  PointsAnnotationType::LINE_LIST,
];

/// Type of points annotation
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct PointsAnnotationType(pub u8);
#[allow(non_upper_case_globals)]
impl PointsAnnotationType {
  /// Unknown points annotation type
  pub const UNKNOWN: Self = Self(0);
  /// Individual points: 0, 1, 2, ...
  pub const POINTS: Self = Self(1);
  /// Closed polygon: 0-1, 1-2, ..., (n-1)-n, n-0
  pub const LINE_LOOP: Self = Self(2);
  /// Connected line segments: 0-1, 1-2, ..., (n-1)-n
  pub const LINE_STRIP: Self = Self(3);
  /// Individual line segments: 0-1, 2-3, 4-5, ...
  pub const LINE_LIST: Self = Self(4);

  pub const ENUM_MIN: u8 = 0;
  pub const ENUM_MAX: u8 = 4;
  pub const ENUM_VALUES: &'static [Self] = &[
    Self::UNKNOWN,
    Self::POINTS,
    Self::LINE_LOOP,
    Self::LINE_STRIP,
    Self::LINE_LIST,
  ];
  /// Returns the variant's name or "" if unknown.
  pub fn variant_name(self) -> Option<&'static str> {
    match self {
      Self::UNKNOWN => Some("UNKNOWN"),
      Self::POINTS => Some("POINTS"),
      Self::LINE_LOOP => Some("LINE_LOOP"),
      Self::LINE_STRIP => Some("LINE_STRIP"),
      Self::LINE_LIST => Some("LINE_LIST"),
      _ => None,
    }
  }
}
impl ::core::fmt::Debug for PointsAnnotationType {
  fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
    if let Some(name) = self.variant_name() {
      f.write_str(name)
    } else {
      f.write_fmt(format_args!("<UNKNOWN {:?}>", self.0))
    }
  }
}
impl<'a> ::flatbuffers::Follow<'a> for PointsAnnotationType {
  type Inner = Self;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    let b = unsafe { ::flatbuffers::read_scalar_at::<u8>(buf, loc) };
    Self(b)
  }
}

impl ::flatbuffers::Push for PointsAnnotationType {
    type Output = PointsAnnotationType;
    #[inline]
    unsafe fn push(&self, dst: &mut [u8], _written_len: usize) {
        unsafe { ::flatbuffers::emplace_scalar::<u8>(dst, self.0) };
    }
}

impl ::flatbuffers::EndianScalar for PointsAnnotationType {
  type Scalar = u8;
  #[inline]
  fn to_little_endian(self) -> u8 {
    self.0.to_le()
  }
  #[inline]
  #[allow(clippy::wrong_self_convention)]
  fn from_little_endian(v: u8) -> Self {
    let b = u8::from_le(v);
    Self(b)
  }
}

impl<'a> ::flatbuffers::Verifiable for PointsAnnotationType {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    u8::run_verifier(v, pos)
  }
}

impl ::flatbuffers::SimpleToVerifyInSlice for PointsAnnotationType {}
pub enum PointsAnnotationOffset {}
#[derive(Copy, Clone, PartialEq)]

/// An array of points on a 2D image
pub struct PointsAnnotation<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for PointsAnnotation<'a> {
  type Inner = PointsAnnotation<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> PointsAnnotation<'a> {
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
  pub const VT_TYPE_: ::flatbuffers::VOffsetT = 6;
  pub const VT_POINTS: ::flatbuffers::VOffsetT = 8;
  pub const VT_OUTLINE_COLOR: ::flatbuffers::VOffsetT = 10;
  pub const VT_OUTLINE_COLORS: ::flatbuffers::VOffsetT = 12;
  pub const VT_FILL_COLOR: ::flatbuffers::VOffsetT = 14;
  pub const VT_THICKNESS: ::flatbuffers::VOffsetT = 16;
  pub const VT_METADATA: ::flatbuffers::VOffsetT = 18;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    PointsAnnotation { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args PointsAnnotationArgs<'args>
  ) -> ::flatbuffers::WIPOffset<PointsAnnotation<'bldr>> {
    let mut builder = PointsAnnotationBuilder::new(_fbb);
    builder.add_thickness(args.thickness);
    if let Some(x) = args.metadata { builder.add_metadata(x); }
    if let Some(x) = args.fill_color { builder.add_fill_color(x); }
    if let Some(x) = args.outline_colors { builder.add_outline_colors(x); }
    if let Some(x) = args.outline_color { builder.add_outline_color(x); }
    if let Some(x) = args.points { builder.add_points(x); }
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    builder.add_type_(args.type_);
    builder.finish()
  }


  /// Timestamp of annotation
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(PointsAnnotation::VT_TIMESTAMP, None)}
  }
  /// Type of points annotation to draw
  #[inline]
  pub fn type_(&self) -> PointsAnnotationType {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<PointsAnnotationType>(PointsAnnotation::VT_TYPE_, Some(PointsAnnotationType::UNKNOWN)).unwrap()}
  }
  /// Points in 2D image coordinates (pixels).
  /// These coordinates use the top-left corner of the top-left pixel of the image as the origin.
  #[inline]
  pub fn points(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<Point2<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<Point2>>>>(PointsAnnotation::VT_POINTS, None)}
  }
  /// Outline color
  #[inline]
  pub fn outline_color(&self) -> Option<Color<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Color>>(PointsAnnotation::VT_OUTLINE_COLOR, None)}
  }
  /// Per-point colors, if `type` is `POINTS`, or per-segment stroke colors, if `type` is `LINE_LIST`, `LINE_STRIP` or `LINE_LOOP`.
  #[inline]
  pub fn outline_colors(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<Color<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<Color>>>>(PointsAnnotation::VT_OUTLINE_COLORS, None)}
  }
  /// Fill color
  #[inline]
  pub fn fill_color(&self) -> Option<Color<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Color>>(PointsAnnotation::VT_FILL_COLOR, None)}
  }
  /// Stroke thickness in pixels
  #[inline]
  pub fn thickness(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(PointsAnnotation::VT_THICKNESS, Some(0.0)).unwrap()}
  }
  /// Additional user-provided metadata associated with this annotation. Keys must be unique.
  #[inline]
  pub fn metadata(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<KeyValuePair<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<KeyValuePair>>>>(PointsAnnotation::VT_METADATA, None)}
  }
}

impl ::flatbuffers::Verifiable for PointsAnnotation<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<PointsAnnotationType>("type_", Self::VT_TYPE_, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<Point2>>>>("points", Self::VT_POINTS, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Color>>("outline_color", Self::VT_OUTLINE_COLOR, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<Color>>>>("outline_colors", Self::VT_OUTLINE_COLORS, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Color>>("fill_color", Self::VT_FILL_COLOR, false)?
     .visit_field::<f64>("thickness", Self::VT_THICKNESS, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<KeyValuePair>>>>("metadata", Self::VT_METADATA, false)?
     .finish();
    Ok(())
  }
}
pub struct PointsAnnotationArgs<'a> {
    pub timestamp: Option<&'a Time>,
    pub type_: PointsAnnotationType,
    pub points: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<Point2<'a>>>>>,
    pub outline_color: Option<::flatbuffers::WIPOffset<Color<'a>>>,
    pub outline_colors: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<Color<'a>>>>>,
    pub fill_color: Option<::flatbuffers::WIPOffset<Color<'a>>>,
    pub thickness: f64,
    pub metadata: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<KeyValuePair<'a>>>>>,
}
impl<'a> Default for PointsAnnotationArgs<'a> {
  #[inline]
  fn default() -> Self {
    PointsAnnotationArgs {
      timestamp: None,
      type_: PointsAnnotationType::UNKNOWN,
      points: None,
      outline_color: None,
      outline_colors: None,
      fill_color: None,
      thickness: 0.0,
      metadata: None,
    }
  }
}

pub struct PointsAnnotationBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> PointsAnnotationBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(PointsAnnotation::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn add_type_(&mut self, type_: PointsAnnotationType) {
    self.fbb_.push_slot::<PointsAnnotationType>(PointsAnnotation::VT_TYPE_, type_, PointsAnnotationType::UNKNOWN);
  }
  #[inline]
  pub fn add_points(&mut self, points: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<Point2<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(PointsAnnotation::VT_POINTS, points);
  }
  #[inline]
  pub fn add_outline_color(&mut self, outline_color: ::flatbuffers::WIPOffset<Color<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Color>>(PointsAnnotation::VT_OUTLINE_COLOR, outline_color);
  }
  #[inline]
  pub fn add_outline_colors(&mut self, outline_colors: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<Color<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(PointsAnnotation::VT_OUTLINE_COLORS, outline_colors);
  }
  #[inline]
  pub fn add_fill_color(&mut self, fill_color: ::flatbuffers::WIPOffset<Color<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Color>>(PointsAnnotation::VT_FILL_COLOR, fill_color);
  }
  #[inline]
  pub fn add_thickness(&mut self, thickness: f64) {
    self.fbb_.push_slot::<f64>(PointsAnnotation::VT_THICKNESS, thickness, 0.0);
  }
  #[inline]
  pub fn add_metadata(&mut self, metadata: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<KeyValuePair<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(PointsAnnotation::VT_METADATA, metadata);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> PointsAnnotationBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    PointsAnnotationBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<PointsAnnotation<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for PointsAnnotation<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("PointsAnnotation");
      ds.field("timestamp", &self.timestamp());
      ds.field("type_", &self.type_());
      ds.field("points", &self.points());
      ds.field("outline_color", &self.outline_color());
      ds.field("outline_colors", &self.outline_colors());
      ds.field("fill_color", &self.fill_color());
      ds.field("thickness", &self.thickness());
      ds.field("metadata", &self.metadata());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `PointsAnnotation`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_points_annotation_unchecked`.
pub fn root_as_points_annotation(buf: &[u8]) -> Result<PointsAnnotation<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<PointsAnnotation>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `PointsAnnotation` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_points_annotation_unchecked`.
pub fn size_prefixed_root_as_points_annotation(buf: &[u8]) -> Result<PointsAnnotation<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<PointsAnnotation>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `PointsAnnotation` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_points_annotation_unchecked`.
pub fn root_as_points_annotation_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<PointsAnnotation<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<PointsAnnotation<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `PointsAnnotation` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_points_annotation_unchecked`.
pub fn size_prefixed_root_as_points_annotation_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<PointsAnnotation<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<PointsAnnotation<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a PointsAnnotation and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `PointsAnnotation`.
pub unsafe fn root_as_points_annotation_unchecked(buf: &[u8]) -> PointsAnnotation<'_> {
  unsafe { ::flatbuffers::root_unchecked::<PointsAnnotation>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed PointsAnnotation and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `PointsAnnotation`.
pub unsafe fn size_prefixed_root_as_points_annotation_unchecked(buf: &[u8]) -> PointsAnnotation<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<PointsAnnotation>(buf) }
}
#[inline]
pub fn finish_points_annotation_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<PointsAnnotation<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_points_annotation_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<PointsAnnotation<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from PoseInFrame_generated.rs =====



pub enum PoseInFrameOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A timestamped pose for an object or reference frame in 3D space
pub struct PoseInFrame<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for PoseInFrame<'a> {
  type Inner = PoseInFrame<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> PoseInFrame<'a> {
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
  pub const VT_FRAME_ID: ::flatbuffers::VOffsetT = 6;
  pub const VT_POSE: ::flatbuffers::VOffsetT = 8;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    PoseInFrame { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args PoseInFrameArgs<'args>
  ) -> ::flatbuffers::WIPOffset<PoseInFrame<'bldr>> {
    let mut builder = PoseInFrameBuilder::new(_fbb);
    if let Some(x) = args.pose { builder.add_pose(x); }
    if let Some(x) = args.frame_id { builder.add_frame_id(x); }
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    builder.finish()
  }


  /// Timestamp of pose
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(PoseInFrame::VT_TIMESTAMP, None)}
  }
  /// Frame of reference for pose position and orientation
  #[inline]
  pub fn frame_id(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(PoseInFrame::VT_FRAME_ID, None)}
  }
  /// Pose in 3D space
  #[inline]
  pub fn pose(&self) -> Option<Pose<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Pose>>(PoseInFrame::VT_POSE, None)}
  }
}

impl ::flatbuffers::Verifiable for PoseInFrame<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("frame_id", Self::VT_FRAME_ID, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Pose>>("pose", Self::VT_POSE, false)?
     .finish();
    Ok(())
  }
}
pub struct PoseInFrameArgs<'a> {
    pub timestamp: Option<&'a Time>,
    pub frame_id: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub pose: Option<::flatbuffers::WIPOffset<Pose<'a>>>,
}
impl<'a> Default for PoseInFrameArgs<'a> {
  #[inline]
  fn default() -> Self {
    PoseInFrameArgs {
      timestamp: None,
      frame_id: None,
      pose: None,
    }
  }
}

pub struct PoseInFrameBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> PoseInFrameBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(PoseInFrame::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn add_frame_id(&mut self, frame_id: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(PoseInFrame::VT_FRAME_ID, frame_id);
  }
  #[inline]
  pub fn add_pose(&mut self, pose: ::flatbuffers::WIPOffset<Pose<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Pose>>(PoseInFrame::VT_POSE, pose);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> PoseInFrameBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    PoseInFrameBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<PoseInFrame<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for PoseInFrame<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("PoseInFrame");
      ds.field("timestamp", &self.timestamp());
      ds.field("frame_id", &self.frame_id());
      ds.field("pose", &self.pose());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `PoseInFrame`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_pose_in_frame_unchecked`.
pub fn root_as_pose_in_frame(buf: &[u8]) -> Result<PoseInFrame<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<PoseInFrame>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `PoseInFrame` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_pose_in_frame_unchecked`.
pub fn size_prefixed_root_as_pose_in_frame(buf: &[u8]) -> Result<PoseInFrame<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<PoseInFrame>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `PoseInFrame` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_pose_in_frame_unchecked`.
pub fn root_as_pose_in_frame_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<PoseInFrame<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<PoseInFrame<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `PoseInFrame` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_pose_in_frame_unchecked`.
pub fn size_prefixed_root_as_pose_in_frame_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<PoseInFrame<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<PoseInFrame<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a PoseInFrame and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `PoseInFrame`.
pub unsafe fn root_as_pose_in_frame_unchecked(buf: &[u8]) -> PoseInFrame<'_> {
  unsafe { ::flatbuffers::root_unchecked::<PoseInFrame>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed PoseInFrame and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `PoseInFrame`.
pub unsafe fn size_prefixed_root_as_pose_in_frame_unchecked(buf: &[u8]) -> PoseInFrame<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<PoseInFrame>(buf) }
}
#[inline]
pub fn finish_pose_in_frame_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<PoseInFrame<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_pose_in_frame_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<PoseInFrame<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from Pose_generated.rs =====



pub enum PoseOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A position and orientation for an object or reference frame in 3D space
pub struct Pose<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for Pose<'a> {
  type Inner = Pose<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> Pose<'a> {
  pub const VT_POSITION: ::flatbuffers::VOffsetT = 4;
  pub const VT_ORIENTATION: ::flatbuffers::VOffsetT = 6;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    Pose { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args PoseArgs<'args>
  ) -> ::flatbuffers::WIPOffset<Pose<'bldr>> {
    let mut builder = PoseBuilder::new(_fbb);
    if let Some(x) = args.orientation { builder.add_orientation(x); }
    if let Some(x) = args.position { builder.add_position(x); }
    builder.finish()
  }


  /// Point denoting position in 3D space
  #[inline]
  pub fn position(&self) -> Option<Vector3<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Vector3>>(Pose::VT_POSITION, None)}
  }
  /// Quaternion denoting orientation in 3D space
  #[inline]
  pub fn orientation(&self) -> Option<Quaternion<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Quaternion>>(Pose::VT_ORIENTATION, None)}
  }
}

impl ::flatbuffers::Verifiable for Pose<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Vector3>>("position", Self::VT_POSITION, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Quaternion>>("orientation", Self::VT_ORIENTATION, false)?
     .finish();
    Ok(())
  }
}
pub struct PoseArgs<'a> {
    pub position: Option<::flatbuffers::WIPOffset<Vector3<'a>>>,
    pub orientation: Option<::flatbuffers::WIPOffset<Quaternion<'a>>>,
}
impl<'a> Default for PoseArgs<'a> {
  #[inline]
  fn default() -> Self {
    PoseArgs {
      position: None,
      orientation: None,
    }
  }
}

pub struct PoseBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> PoseBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_position(&mut self, position: ::flatbuffers::WIPOffset<Vector3<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Vector3>>(Pose::VT_POSITION, position);
  }
  #[inline]
  pub fn add_orientation(&mut self, orientation: ::flatbuffers::WIPOffset<Quaternion<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Quaternion>>(Pose::VT_ORIENTATION, orientation);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> PoseBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    PoseBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<Pose<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for Pose<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("Pose");
      ds.field("position", &self.position());
      ds.field("orientation", &self.orientation());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `Pose`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_pose_unchecked`.
pub fn root_as_pose(buf: &[u8]) -> Result<Pose<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<Pose>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `Pose` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_pose_unchecked`.
pub fn size_prefixed_root_as_pose(buf: &[u8]) -> Result<Pose<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<Pose>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `Pose` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_pose_unchecked`.
pub fn root_as_pose_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<Pose<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<Pose<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `Pose` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_pose_unchecked`.
pub fn size_prefixed_root_as_pose_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<Pose<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<Pose<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a Pose and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `Pose`.
pub unsafe fn root_as_pose_unchecked(buf: &[u8]) -> Pose<'_> {
  unsafe { ::flatbuffers::root_unchecked::<Pose>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed Pose and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `Pose`.
pub unsafe fn size_prefixed_root_as_pose_unchecked(buf: &[u8]) -> Pose<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<Pose>(buf) }
}
#[inline]
pub fn finish_pose_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<Pose<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_pose_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<Pose<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from PosesInFrame_generated.rs =====



pub enum PosesInFrameOffset {}
#[derive(Copy, Clone, PartialEq)]

/// An array of timestamped poses for an object or reference frame in 3D space
pub struct PosesInFrame<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for PosesInFrame<'a> {
  type Inner = PosesInFrame<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> PosesInFrame<'a> {
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
  pub const VT_FRAME_ID: ::flatbuffers::VOffsetT = 6;
  pub const VT_POSES: ::flatbuffers::VOffsetT = 8;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    PosesInFrame { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args PosesInFrameArgs<'args>
  ) -> ::flatbuffers::WIPOffset<PosesInFrame<'bldr>> {
    let mut builder = PosesInFrameBuilder::new(_fbb);
    if let Some(x) = args.poses { builder.add_poses(x); }
    if let Some(x) = args.frame_id { builder.add_frame_id(x); }
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    builder.finish()
  }


  /// Timestamp of pose
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(PosesInFrame::VT_TIMESTAMP, None)}
  }
  /// Frame of reference for pose position and orientation
  #[inline]
  pub fn frame_id(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(PosesInFrame::VT_FRAME_ID, None)}
  }
  /// Poses in 3D space
  #[inline]
  pub fn poses(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<Pose<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<Pose>>>>(PosesInFrame::VT_POSES, None)}
  }
}

impl ::flatbuffers::Verifiable for PosesInFrame<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("frame_id", Self::VT_FRAME_ID, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<Pose>>>>("poses", Self::VT_POSES, false)?
     .finish();
    Ok(())
  }
}
pub struct PosesInFrameArgs<'a> {
    pub timestamp: Option<&'a Time>,
    pub frame_id: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub poses: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<Pose<'a>>>>>,
}
impl<'a> Default for PosesInFrameArgs<'a> {
  #[inline]
  fn default() -> Self {
    PosesInFrameArgs {
      timestamp: None,
      frame_id: None,
      poses: None,
    }
  }
}

pub struct PosesInFrameBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> PosesInFrameBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(PosesInFrame::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn add_frame_id(&mut self, frame_id: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(PosesInFrame::VT_FRAME_ID, frame_id);
  }
  #[inline]
  pub fn add_poses(&mut self, poses: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<Pose<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(PosesInFrame::VT_POSES, poses);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> PosesInFrameBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    PosesInFrameBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<PosesInFrame<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for PosesInFrame<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("PosesInFrame");
      ds.field("timestamp", &self.timestamp());
      ds.field("frame_id", &self.frame_id());
      ds.field("poses", &self.poses());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `PosesInFrame`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_poses_in_frame_unchecked`.
pub fn root_as_poses_in_frame(buf: &[u8]) -> Result<PosesInFrame<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<PosesInFrame>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `PosesInFrame` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_poses_in_frame_unchecked`.
pub fn size_prefixed_root_as_poses_in_frame(buf: &[u8]) -> Result<PosesInFrame<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<PosesInFrame>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `PosesInFrame` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_poses_in_frame_unchecked`.
pub fn root_as_poses_in_frame_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<PosesInFrame<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<PosesInFrame<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `PosesInFrame` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_poses_in_frame_unchecked`.
pub fn size_prefixed_root_as_poses_in_frame_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<PosesInFrame<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<PosesInFrame<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a PosesInFrame and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `PosesInFrame`.
pub unsafe fn root_as_poses_in_frame_unchecked(buf: &[u8]) -> PosesInFrame<'_> {
  unsafe { ::flatbuffers::root_unchecked::<PosesInFrame>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed PosesInFrame and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `PosesInFrame`.
pub unsafe fn size_prefixed_root_as_poses_in_frame_unchecked(buf: &[u8]) -> PosesInFrame<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<PosesInFrame>(buf) }
}
#[inline]
pub fn finish_poses_in_frame_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<PosesInFrame<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_poses_in_frame_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<PosesInFrame<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from Quaternion_generated.rs =====



pub enum QuaternionOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A [quaternion](https://eater.net/quaternions) representing a rotation in 3D space
pub struct Quaternion<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for Quaternion<'a> {
  type Inner = Quaternion<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> Quaternion<'a> {
  pub const VT_X: ::flatbuffers::VOffsetT = 4;
  pub const VT_Y: ::flatbuffers::VOffsetT = 6;
  pub const VT_Z: ::flatbuffers::VOffsetT = 8;
  pub const VT_W: ::flatbuffers::VOffsetT = 10;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    Quaternion { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args QuaternionArgs
  ) -> ::flatbuffers::WIPOffset<Quaternion<'bldr>> {
    let mut builder = QuaternionBuilder::new(_fbb);
    builder.add_w(args.w);
    builder.add_z(args.z);
    builder.add_y(args.y);
    builder.add_x(args.x);
    builder.finish()
  }


  /// x value
  #[inline]
  pub fn x(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(Quaternion::VT_X, Some(0.0)).unwrap()}
  }
  /// y value
  #[inline]
  pub fn y(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(Quaternion::VT_Y, Some(0.0)).unwrap()}
  }
  /// z value
  #[inline]
  pub fn z(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(Quaternion::VT_Z, Some(0.0)).unwrap()}
  }
  /// w value
  #[inline]
  pub fn w(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(Quaternion::VT_W, Some(1.0)).unwrap()}
  }
}

impl ::flatbuffers::Verifiable for Quaternion<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<f64>("x", Self::VT_X, false)?
     .visit_field::<f64>("y", Self::VT_Y, false)?
     .visit_field::<f64>("z", Self::VT_Z, false)?
     .visit_field::<f64>("w", Self::VT_W, false)?
     .finish();
    Ok(())
  }
}
pub struct QuaternionArgs {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}
impl<'a> Default for QuaternionArgs {
  #[inline]
  fn default() -> Self {
    QuaternionArgs {
      x: 0.0,
      y: 0.0,
      z: 0.0,
      w: 1.0,
    }
  }
}

pub struct QuaternionBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> QuaternionBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_x(&mut self, x: f64) {
    self.fbb_.push_slot::<f64>(Quaternion::VT_X, x, 0.0);
  }
  #[inline]
  pub fn add_y(&mut self, y: f64) {
    self.fbb_.push_slot::<f64>(Quaternion::VT_Y, y, 0.0);
  }
  #[inline]
  pub fn add_z(&mut self, z: f64) {
    self.fbb_.push_slot::<f64>(Quaternion::VT_Z, z, 0.0);
  }
  #[inline]
  pub fn add_w(&mut self, w: f64) {
    self.fbb_.push_slot::<f64>(Quaternion::VT_W, w, 1.0);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> QuaternionBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    QuaternionBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<Quaternion<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for Quaternion<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("Quaternion");
      ds.field("x", &self.x());
      ds.field("y", &self.y());
      ds.field("z", &self.z());
      ds.field("w", &self.w());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `Quaternion`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_quaternion_unchecked`.
pub fn root_as_quaternion(buf: &[u8]) -> Result<Quaternion<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<Quaternion>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `Quaternion` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_quaternion_unchecked`.
pub fn size_prefixed_root_as_quaternion(buf: &[u8]) -> Result<Quaternion<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<Quaternion>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `Quaternion` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_quaternion_unchecked`.
pub fn root_as_quaternion_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<Quaternion<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<Quaternion<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `Quaternion` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_quaternion_unchecked`.
pub fn size_prefixed_root_as_quaternion_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<Quaternion<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<Quaternion<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a Quaternion and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `Quaternion`.
pub unsafe fn root_as_quaternion_unchecked(buf: &[u8]) -> Quaternion<'_> {
  unsafe { ::flatbuffers::root_unchecked::<Quaternion>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed Quaternion and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `Quaternion`.
pub unsafe fn size_prefixed_root_as_quaternion_unchecked(buf: &[u8]) -> Quaternion<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<Quaternion>(buf) }
}
#[inline]
pub fn finish_quaternion_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<Quaternion<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_quaternion_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<Quaternion<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from RawAudio_generated.rs =====



pub enum RawAudioOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A single block of an audio bitstream
pub struct RawAudio<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for RawAudio<'a> {
  type Inner = RawAudio<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> RawAudio<'a> {
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
  pub const VT_DATA: ::flatbuffers::VOffsetT = 6;
  pub const VT_FORMAT: ::flatbuffers::VOffsetT = 8;
  pub const VT_SAMPLE_RATE: ::flatbuffers::VOffsetT = 10;
  pub const VT_NUMBER_OF_CHANNELS: ::flatbuffers::VOffsetT = 12;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    RawAudio { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args RawAudioArgs<'args>
  ) -> ::flatbuffers::WIPOffset<RawAudio<'bldr>> {
    let mut builder = RawAudioBuilder::new(_fbb);
    builder.add_number_of_channels(args.number_of_channels);
    builder.add_sample_rate(args.sample_rate);
    if let Some(x) = args.format { builder.add_format(x); }
    if let Some(x) = args.data { builder.add_data(x); }
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    builder.finish()
  }


  /// Timestamp of the start of the audio block
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(RawAudio::VT_TIMESTAMP, None)}
  }
  /// Audio data. The samples in the data must be interleaved and little-endian
  #[inline]
  pub fn data(&self) -> Option<::flatbuffers::Vector<'a, u8>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, u8>>>(RawAudio::VT_DATA, None)}
  }
  /// Audio format. Only 'pcm-s16' is currently supported
  #[inline]
  pub fn format(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(RawAudio::VT_FORMAT, None)}
  }
  /// Sample rate in Hz
  #[inline]
  pub fn sample_rate(&self) -> u32 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<u32>(RawAudio::VT_SAMPLE_RATE, Some(0)).unwrap()}
  }
  /// Number of channels in the audio block
  #[inline]
  pub fn number_of_channels(&self) -> u32 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<u32>(RawAudio::VT_NUMBER_OF_CHANNELS, Some(0)).unwrap()}
  }
}

impl ::flatbuffers::Verifiable for RawAudio<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, u8>>>("data", Self::VT_DATA, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("format", Self::VT_FORMAT, false)?
     .visit_field::<u32>("sample_rate", Self::VT_SAMPLE_RATE, false)?
     .visit_field::<u32>("number_of_channels", Self::VT_NUMBER_OF_CHANNELS, false)?
     .finish();
    Ok(())
  }
}
pub struct RawAudioArgs<'a> {
    pub timestamp: Option<&'a Time>,
    pub data: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, u8>>>,
    pub format: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub sample_rate: u32,
    pub number_of_channels: u32,
}
impl<'a> Default for RawAudioArgs<'a> {
  #[inline]
  fn default() -> Self {
    RawAudioArgs {
      timestamp: None,
      data: None,
      format: None,
      sample_rate: 0,
      number_of_channels: 0,
    }
  }
}

pub struct RawAudioBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> RawAudioBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(RawAudio::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn add_data(&mut self, data: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , u8>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(RawAudio::VT_DATA, data);
  }
  #[inline]
  pub fn add_format(&mut self, format: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(RawAudio::VT_FORMAT, format);
  }
  #[inline]
  pub fn add_sample_rate(&mut self, sample_rate: u32) {
    self.fbb_.push_slot::<u32>(RawAudio::VT_SAMPLE_RATE, sample_rate, 0);
  }
  #[inline]
  pub fn add_number_of_channels(&mut self, number_of_channels: u32) {
    self.fbb_.push_slot::<u32>(RawAudio::VT_NUMBER_OF_CHANNELS, number_of_channels, 0);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> RawAudioBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    RawAudioBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<RawAudio<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for RawAudio<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("RawAudio");
      ds.field("timestamp", &self.timestamp());
      ds.field("data", &self.data());
      ds.field("format", &self.format());
      ds.field("sample_rate", &self.sample_rate());
      ds.field("number_of_channels", &self.number_of_channels());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `RawAudio`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_raw_audio_unchecked`.
pub fn root_as_raw_audio(buf: &[u8]) -> Result<RawAudio<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<RawAudio>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `RawAudio` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_raw_audio_unchecked`.
pub fn size_prefixed_root_as_raw_audio(buf: &[u8]) -> Result<RawAudio<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<RawAudio>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `RawAudio` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_raw_audio_unchecked`.
pub fn root_as_raw_audio_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<RawAudio<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<RawAudio<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `RawAudio` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_raw_audio_unchecked`.
pub fn size_prefixed_root_as_raw_audio_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<RawAudio<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<RawAudio<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a RawAudio and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `RawAudio`.
pub unsafe fn root_as_raw_audio_unchecked(buf: &[u8]) -> RawAudio<'_> {
  unsafe { ::flatbuffers::root_unchecked::<RawAudio>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed RawAudio and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `RawAudio`.
pub unsafe fn size_prefixed_root_as_raw_audio_unchecked(buf: &[u8]) -> RawAudio<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<RawAudio>(buf) }
}
#[inline]
pub fn finish_raw_audio_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<RawAudio<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_raw_audio_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<RawAudio<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from RawImage_generated.rs =====



pub enum RawImageOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A raw image
pub struct RawImage<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for RawImage<'a> {
  type Inner = RawImage<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> RawImage<'a> {
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
  pub const VT_FRAME_ID: ::flatbuffers::VOffsetT = 6;
  pub const VT_WIDTH: ::flatbuffers::VOffsetT = 8;
  pub const VT_HEIGHT: ::flatbuffers::VOffsetT = 10;
  pub const VT_ENCODING: ::flatbuffers::VOffsetT = 12;
  pub const VT_STEP: ::flatbuffers::VOffsetT = 14;
  pub const VT_DATA: ::flatbuffers::VOffsetT = 16;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    RawImage { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args RawImageArgs<'args>
  ) -> ::flatbuffers::WIPOffset<RawImage<'bldr>> {
    let mut builder = RawImageBuilder::new(_fbb);
    if let Some(x) = args.data { builder.add_data(x); }
    builder.add_step(args.step);
    if let Some(x) = args.encoding { builder.add_encoding(x); }
    builder.add_height(args.height);
    builder.add_width(args.width);
    if let Some(x) = args.frame_id { builder.add_frame_id(x); }
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    builder.finish()
  }


  /// Timestamp of image
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(RawImage::VT_TIMESTAMP, None)}
  }
  /// Frame of reference for the image. The origin of the frame is the optical center of the camera. +x points to the right in the image, +y points down, and +z points into the plane of the image.
  #[inline]
  pub fn frame_id(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(RawImage::VT_FRAME_ID, None)}
  }
  /// Image width in pixels
  #[inline]
  pub fn width(&self) -> u32 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<u32>(RawImage::VT_WIDTH, Some(0)).unwrap()}
  }
  /// Image height in pixels
  #[inline]
  pub fn height(&self) -> u32 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<u32>(RawImage::VT_HEIGHT, Some(0)).unwrap()}
  }
  /// Encoding of the raw image data. See the `data` field description for supported values.
  #[inline]
  pub fn encoding(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(RawImage::VT_ENCODING, None)}
  }
  /// Byte length of a single row. This is usually some multiple of `width` depending on the encoding, but can be greater to incorporate padding.
  #[inline]
  pub fn step(&self) -> u32 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<u32>(RawImage::VT_STEP, Some(0)).unwrap()}
  }
  /// Raw image data.
  /// 
  /// For each `encoding` value, the `data` field contains image pixel data serialized as follows:
  /// 
  /// - `yuv422` or `uyvy`:
  ///   - Pixel colors are decomposed into [Y'UV](https://en.wikipedia.org/wiki/Y%E2%80%B2UV) channels.
  ///   - Pixel channel values are represented as unsigned 8-bit integers.
  ///   - U and V values are shared between horizontal pairs of pixels. Each pair of output pixels is serialized as [U, Y1, V, Y2].
  ///   - `step` must be greater than or equal to `width` * 2.
  /// - `yuv422_yuy2` or  `yuyv`:
  ///   - Pixel colors are decomposed into [Y'UV](https://en.wikipedia.org/wiki/Y%E2%80%B2UV) channels.
  ///   - Pixel channel values are represented as unsigned 8-bit integers.
  ///   - U and V values are shared between horizontal pairs of pixels. Each pair of output pixels is encoded as [Y1, U, Y2, V].
  ///   - `step` must be greater than or equal to `width` * 2.
  /// - `nv12`:
  ///   - Pixel colors are decomposed into [Y'UV](https://en.wikipedia.org/wiki/Y%E2%80%B2UV) channels using 4:2:0 chroma subsampling. The data is stored in [NV12](https://www.kernel.org/doc/html/v4.10/media/uapi/v4l/pixfmt-nv12.html) semi-planar layout with two contiguous planes: a Y (luma) plane followed by an interleaved UV (chroma) plane.
  ///   - All channel values are represented as unsigned 8-bit integers.
  ///   - Both planes use `step` as their row stride.
  ///   - The Y plane contains one luma value per pixel (`step` * `height` bytes).
  ///   - The UV plane contains interleaved U, V chroma pairs, subsampled by a factor of 2 in both dimensions (`width`/2 pairs per row, `height`/2 rows, `step` * `height`/2 bytes). Each U, V pair is shared by a 2x2 block of pixels.
  ///   - `width` and `height` must be even.
  ///   - `step` must be greater than or equal to `width`.
  ///   - Total `data` length is `step` * `height` * 3/2 bytes.
  /// - `rgb8`:
  ///   - Pixel colors are decomposed into Red, Green, and Blue channels.
  ///   - Pixel channel values are represented as unsigned 8-bit integers.
  ///   - Each output pixel is serialized as [R, G, B].
  ///   - `step` must be greater than or equal to `width` * 3.
  /// - `rgba8`:
  ///   - Pixel colors are decomposed into Red, Green, Blue, and Alpha channels.
  ///   - Pixel channel values are represented as unsigned 8-bit integers.
  ///   - Each output pixel is serialized as [R, G, B, Alpha].
  ///   - `step` must be greater than or equal to `width` * 4.
  /// - `bgr8` or `8UC3`:
  ///   - Pixel colors are decomposed into Blue, Green, and Red channels.
  ///   - Pixel channel values are represented as unsigned 8-bit integers.
  ///   - Each output pixel is serialized as [B, G, R].
  ///   - `step` must be greater than or equal to `width` * 3.
  /// - `bgra8`:
  ///   - Pixel colors are decomposed into Blue, Green, Red, and Alpha channels.
  ///   - Pixel channel values are represented as unsigned 8-bit integers.
  ///   - Each output pixel is encoded as [B, G, R, Alpha].
  ///   - `step` must be greater than or equal to `width` * 4.
  /// - `32FC1`:
  ///   - Pixel brightness is represented as a single-channel, 32-bit little-endian IEEE 754 floating-point value, ranging from 0.0 (black) to 1.0 (white).
  ///   - `step` must be greater than or equal to `width` * 4.
  /// - `bayer_rggb8`, `bayer_bggr8`, `bayer_gbrg8`, or `bayer_grbg8`:
  ///   - Pixel colors are decomposed into Red, Blue and Green channels.
  ///   - Pixel channel values are represented as unsigned 8-bit integers, and serialized in a 2x2 bayer filter pattern.
  ///   - The order of the four letters after `bayer_` determine the layout, so for `bayer_wxyz8` the pattern is:
  ///   ```text
  ///   w | x
  ///   - + -
  ///   y | z
  ///   ```
  ///   - `step` must be greater than or equal to `width`.
  /// - `mono8` or `8UC1`:
  ///   - Pixel brightness is represented as unsigned 8-bit integers.
  ///   - `step` must be greater than or equal to `width`.
  /// - `mono16` or `16UC1`:
  ///   - Pixel brightness is represented as 16-bit unsigned little-endian integers. Rendering of these values is controlled in [Image panel color mode settings](https://docs.foxglove.dev/docs/visualization/panels/image#general).
  ///   - `step` must be greater than or equal to `width` * 2.
  #[inline]
  pub fn data(&self) -> Option<::flatbuffers::Vector<'a, u8>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, u8>>>(RawImage::VT_DATA, None)}
  }
}

impl ::flatbuffers::Verifiable for RawImage<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("frame_id", Self::VT_FRAME_ID, false)?
     .visit_field::<u32>("width", Self::VT_WIDTH, false)?
     .visit_field::<u32>("height", Self::VT_HEIGHT, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("encoding", Self::VT_ENCODING, false)?
     .visit_field::<u32>("step", Self::VT_STEP, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, u8>>>("data", Self::VT_DATA, false)?
     .finish();
    Ok(())
  }
}
pub struct RawImageArgs<'a> {
    pub timestamp: Option<&'a Time>,
    pub frame_id: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub width: u32,
    pub height: u32,
    pub encoding: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub step: u32,
    pub data: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, u8>>>,
}
impl<'a> Default for RawImageArgs<'a> {
  #[inline]
  fn default() -> Self {
    RawImageArgs {
      timestamp: None,
      frame_id: None,
      width: 0,
      height: 0,
      encoding: None,
      step: 0,
      data: None,
    }
  }
}

pub struct RawImageBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> RawImageBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(RawImage::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn add_frame_id(&mut self, frame_id: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(RawImage::VT_FRAME_ID, frame_id);
  }
  #[inline]
  pub fn add_width(&mut self, width: u32) {
    self.fbb_.push_slot::<u32>(RawImage::VT_WIDTH, width, 0);
  }
  #[inline]
  pub fn add_height(&mut self, height: u32) {
    self.fbb_.push_slot::<u32>(RawImage::VT_HEIGHT, height, 0);
  }
  #[inline]
  pub fn add_encoding(&mut self, encoding: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(RawImage::VT_ENCODING, encoding);
  }
  #[inline]
  pub fn add_step(&mut self, step: u32) {
    self.fbb_.push_slot::<u32>(RawImage::VT_STEP, step, 0);
  }
  #[inline]
  pub fn add_data(&mut self, data: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , u8>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(RawImage::VT_DATA, data);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> RawImageBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    RawImageBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<RawImage<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for RawImage<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("RawImage");
      ds.field("timestamp", &self.timestamp());
      ds.field("frame_id", &self.frame_id());
      ds.field("width", &self.width());
      ds.field("height", &self.height());
      ds.field("encoding", &self.encoding());
      ds.field("step", &self.step());
      ds.field("data", &self.data());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `RawImage`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_raw_image_unchecked`.
pub fn root_as_raw_image(buf: &[u8]) -> Result<RawImage<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<RawImage>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `RawImage` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_raw_image_unchecked`.
pub fn size_prefixed_root_as_raw_image(buf: &[u8]) -> Result<RawImage<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<RawImage>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `RawImage` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_raw_image_unchecked`.
pub fn root_as_raw_image_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<RawImage<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<RawImage<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `RawImage` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_raw_image_unchecked`.
pub fn size_prefixed_root_as_raw_image_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<RawImage<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<RawImage<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a RawImage and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `RawImage`.
pub unsafe fn root_as_raw_image_unchecked(buf: &[u8]) -> RawImage<'_> {
  unsafe { ::flatbuffers::root_unchecked::<RawImage>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed RawImage and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `RawImage`.
pub unsafe fn size_prefixed_root_as_raw_image_unchecked(buf: &[u8]) -> RawImage<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<RawImage>(buf) }
}
#[inline]
pub fn finish_raw_image_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<RawImage<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_raw_image_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<RawImage<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from SceneEntityDeletion_generated.rs =====



#[deprecated(since = "2.0.0", note = "Use associated constants instead. This will no longer be generated in 2021.")]
pub const ENUM_MIN_SCENE_ENTITY_DELETION_TYPE: u8 = 0;
#[deprecated(since = "2.0.0", note = "Use associated constants instead. This will no longer be generated in 2021.")]
pub const ENUM_MAX_SCENE_ENTITY_DELETION_TYPE: u8 = 1;
#[deprecated(since = "2.0.0", note = "Use associated constants instead. This will no longer be generated in 2021.")]
#[allow(non_camel_case_types)]
pub const ENUM_VALUES_SCENE_ENTITY_DELETION_TYPE: [SceneEntityDeletionType; 2] = [
  SceneEntityDeletionType::MATCHING_ID,
  SceneEntityDeletionType::ALL,
];

/// An enumeration indicating which entities should match a SceneEntityDeletion command
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct SceneEntityDeletionType(pub u8);
#[allow(non_upper_case_globals)]
impl SceneEntityDeletionType {
  /// Delete the existing entity on the same topic that has the provided `id`
  pub const MATCHING_ID: Self = Self(0);
  /// Delete all existing entities on the same topic
  pub const ALL: Self = Self(1);

  pub const ENUM_MIN: u8 = 0;
  pub const ENUM_MAX: u8 = 1;
  pub const ENUM_VALUES: &'static [Self] = &[
    Self::MATCHING_ID,
    Self::ALL,
  ];
  /// Returns the variant's name or "" if unknown.
  pub fn variant_name(self) -> Option<&'static str> {
    match self {
      Self::MATCHING_ID => Some("MATCHING_ID"),
      Self::ALL => Some("ALL"),
      _ => None,
    }
  }
}
impl ::core::fmt::Debug for SceneEntityDeletionType {
  fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
    if let Some(name) = self.variant_name() {
      f.write_str(name)
    } else {
      f.write_fmt(format_args!("<UNKNOWN {:?}>", self.0))
    }
  }
}
impl<'a> ::flatbuffers::Follow<'a> for SceneEntityDeletionType {
  type Inner = Self;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    let b = unsafe { ::flatbuffers::read_scalar_at::<u8>(buf, loc) };
    Self(b)
  }
}

impl ::flatbuffers::Push for SceneEntityDeletionType {
    type Output = SceneEntityDeletionType;
    #[inline]
    unsafe fn push(&self, dst: &mut [u8], _written_len: usize) {
        unsafe { ::flatbuffers::emplace_scalar::<u8>(dst, self.0) };
    }
}

impl ::flatbuffers::EndianScalar for SceneEntityDeletionType {
  type Scalar = u8;
  #[inline]
  fn to_little_endian(self) -> u8 {
    self.0.to_le()
  }
  #[inline]
  #[allow(clippy::wrong_self_convention)]
  fn from_little_endian(v: u8) -> Self {
    let b = u8::from_le(v);
    Self(b)
  }
}

impl<'a> ::flatbuffers::Verifiable for SceneEntityDeletionType {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    u8::run_verifier(v, pos)
  }
}

impl ::flatbuffers::SimpleToVerifyInSlice for SceneEntityDeletionType {}
pub enum SceneEntityDeletionOffset {}
#[derive(Copy, Clone, PartialEq)]

/// Command to remove previously published entities
pub struct SceneEntityDeletion<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for SceneEntityDeletion<'a> {
  type Inner = SceneEntityDeletion<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> SceneEntityDeletion<'a> {
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
  pub const VT_TYPE_: ::flatbuffers::VOffsetT = 6;
  pub const VT_ID: ::flatbuffers::VOffsetT = 8;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    SceneEntityDeletion { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args SceneEntityDeletionArgs<'args>
  ) -> ::flatbuffers::WIPOffset<SceneEntityDeletion<'bldr>> {
    let mut builder = SceneEntityDeletionBuilder::new(_fbb);
    if let Some(x) = args.id { builder.add_id(x); }
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    builder.add_type_(args.type_);
    builder.finish()
  }


  /// Timestamp of the deletion. Only matching entities earlier than this timestamp will be deleted.
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(SceneEntityDeletion::VT_TIMESTAMP, None)}
  }
  /// Type of deletion action to perform
  #[inline]
  pub fn type_(&self) -> SceneEntityDeletionType {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<SceneEntityDeletionType>(SceneEntityDeletion::VT_TYPE_, Some(SceneEntityDeletionType::MATCHING_ID)).unwrap()}
  }
  /// Identifier which must match if `type` is `MATCHING_ID`.
  #[inline]
  pub fn id(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(SceneEntityDeletion::VT_ID, None)}
  }
}

impl ::flatbuffers::Verifiable for SceneEntityDeletion<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<SceneEntityDeletionType>("type_", Self::VT_TYPE_, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("id", Self::VT_ID, false)?
     .finish();
    Ok(())
  }
}
pub struct SceneEntityDeletionArgs<'a> {
    pub timestamp: Option<&'a Time>,
    pub type_: SceneEntityDeletionType,
    pub id: Option<::flatbuffers::WIPOffset<&'a str>>,
}
impl<'a> Default for SceneEntityDeletionArgs<'a> {
  #[inline]
  fn default() -> Self {
    SceneEntityDeletionArgs {
      timestamp: None,
      type_: SceneEntityDeletionType::MATCHING_ID,
      id: None,
    }
  }
}

pub struct SceneEntityDeletionBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> SceneEntityDeletionBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(SceneEntityDeletion::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn add_type_(&mut self, type_: SceneEntityDeletionType) {
    self.fbb_.push_slot::<SceneEntityDeletionType>(SceneEntityDeletion::VT_TYPE_, type_, SceneEntityDeletionType::MATCHING_ID);
  }
  #[inline]
  pub fn add_id(&mut self, id: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(SceneEntityDeletion::VT_ID, id);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> SceneEntityDeletionBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    SceneEntityDeletionBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<SceneEntityDeletion<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for SceneEntityDeletion<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("SceneEntityDeletion");
      ds.field("timestamp", &self.timestamp());
      ds.field("type_", &self.type_());
      ds.field("id", &self.id());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `SceneEntityDeletion`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_scene_entity_deletion_unchecked`.
pub fn root_as_scene_entity_deletion(buf: &[u8]) -> Result<SceneEntityDeletion<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<SceneEntityDeletion>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `SceneEntityDeletion` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_scene_entity_deletion_unchecked`.
pub fn size_prefixed_root_as_scene_entity_deletion(buf: &[u8]) -> Result<SceneEntityDeletion<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<SceneEntityDeletion>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `SceneEntityDeletion` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_scene_entity_deletion_unchecked`.
pub fn root_as_scene_entity_deletion_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<SceneEntityDeletion<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<SceneEntityDeletion<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `SceneEntityDeletion` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_scene_entity_deletion_unchecked`.
pub fn size_prefixed_root_as_scene_entity_deletion_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<SceneEntityDeletion<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<SceneEntityDeletion<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a SceneEntityDeletion and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `SceneEntityDeletion`.
pub unsafe fn root_as_scene_entity_deletion_unchecked(buf: &[u8]) -> SceneEntityDeletion<'_> {
  unsafe { ::flatbuffers::root_unchecked::<SceneEntityDeletion>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed SceneEntityDeletion and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `SceneEntityDeletion`.
pub unsafe fn size_prefixed_root_as_scene_entity_deletion_unchecked(buf: &[u8]) -> SceneEntityDeletion<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<SceneEntityDeletion>(buf) }
}
#[inline]
pub fn finish_scene_entity_deletion_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<SceneEntityDeletion<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_scene_entity_deletion_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<SceneEntityDeletion<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from SceneEntity_generated.rs =====



pub enum SceneEntityOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A visual element in a 3D scene. An entity may be composed of multiple primitives which all share the same frame of reference.
pub struct SceneEntity<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for SceneEntity<'a> {
  type Inner = SceneEntity<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> SceneEntity<'a> {
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
  pub const VT_FRAME_ID: ::flatbuffers::VOffsetT = 6;
  pub const VT_ID: ::flatbuffers::VOffsetT = 8;
  pub const VT_LIFETIME: ::flatbuffers::VOffsetT = 10;
  pub const VT_FRAME_LOCKED: ::flatbuffers::VOffsetT = 12;
  pub const VT_METADATA: ::flatbuffers::VOffsetT = 14;
  pub const VT_ARROWS: ::flatbuffers::VOffsetT = 16;
  pub const VT_CUBES: ::flatbuffers::VOffsetT = 18;
  pub const VT_SPHERES: ::flatbuffers::VOffsetT = 20;
  pub const VT_CYLINDERS: ::flatbuffers::VOffsetT = 22;
  pub const VT_LINES: ::flatbuffers::VOffsetT = 24;
  pub const VT_TRIANGLES: ::flatbuffers::VOffsetT = 26;
  pub const VT_TEXTS: ::flatbuffers::VOffsetT = 28;
  pub const VT_MODELS: ::flatbuffers::VOffsetT = 30;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    SceneEntity { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args SceneEntityArgs<'args>
  ) -> ::flatbuffers::WIPOffset<SceneEntity<'bldr>> {
    let mut builder = SceneEntityBuilder::new(_fbb);
    if let Some(x) = args.models { builder.add_models(x); }
    if let Some(x) = args.texts { builder.add_texts(x); }
    if let Some(x) = args.triangles { builder.add_triangles(x); }
    if let Some(x) = args.lines { builder.add_lines(x); }
    if let Some(x) = args.cylinders { builder.add_cylinders(x); }
    if let Some(x) = args.spheres { builder.add_spheres(x); }
    if let Some(x) = args.cubes { builder.add_cubes(x); }
    if let Some(x) = args.arrows { builder.add_arrows(x); }
    if let Some(x) = args.metadata { builder.add_metadata(x); }
    if let Some(x) = args.lifetime { builder.add_lifetime(x); }
    if let Some(x) = args.id { builder.add_id(x); }
    if let Some(x) = args.frame_id { builder.add_frame_id(x); }
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    builder.add_frame_locked(args.frame_locked);
    builder.finish()
  }


  /// Timestamp of the entity
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(SceneEntity::VT_TIMESTAMP, None)}
  }
  /// Frame of reference
  #[inline]
  pub fn frame_id(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(SceneEntity::VT_FRAME_ID, None)}
  }
  /// Identifier for the entity. A entity will replace any prior entity on the same topic with the same `id`.
  #[inline]
  pub fn id(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(SceneEntity::VT_ID, None)}
  }
  /// Length of time (relative to `timestamp`) after which the entity should be automatically removed. Zero value indicates the entity should remain visible until it is replaced or deleted.
  #[inline]
  pub fn lifetime(&self) -> Option<&'a Duration> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Duration>(SceneEntity::VT_LIFETIME, None)}
  }
  /// Whether the entity should keep its location in the fixed frame (false) or follow the frame specified in `frame_id` as it moves relative to the fixed frame (true)
  #[inline]
  pub fn frame_locked(&self) -> bool {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<bool>(SceneEntity::VT_FRAME_LOCKED, Some(false)).unwrap()}
  }
  /// Additional user-provided metadata associated with the entity. Keys must be unique.
  #[inline]
  pub fn metadata(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<KeyValuePair<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<KeyValuePair>>>>(SceneEntity::VT_METADATA, None)}
  }
  /// Arrow primitives
  #[inline]
  pub fn arrows(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<ArrowPrimitive<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<ArrowPrimitive>>>>(SceneEntity::VT_ARROWS, None)}
  }
  /// Cube primitives
  #[inline]
  pub fn cubes(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<CubePrimitive<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<CubePrimitive>>>>(SceneEntity::VT_CUBES, None)}
  }
  /// Sphere primitives
  #[inline]
  pub fn spheres(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<SpherePrimitive<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<SpherePrimitive>>>>(SceneEntity::VT_SPHERES, None)}
  }
  /// Cylinder primitives
  #[inline]
  pub fn cylinders(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<CylinderPrimitive<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<CylinderPrimitive>>>>(SceneEntity::VT_CYLINDERS, None)}
  }
  /// Line primitives
  #[inline]
  pub fn lines(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<LinePrimitive<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<LinePrimitive>>>>(SceneEntity::VT_LINES, None)}
  }
  /// Triangle list primitives
  #[inline]
  pub fn triangles(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<TriangleListPrimitive<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<TriangleListPrimitive>>>>(SceneEntity::VT_TRIANGLES, None)}
  }
  /// Text primitives
  #[inline]
  pub fn texts(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<TextPrimitive<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<TextPrimitive>>>>(SceneEntity::VT_TEXTS, None)}
  }
  /// Model primitives
  #[inline]
  pub fn models(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<ModelPrimitive<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<ModelPrimitive>>>>(SceneEntity::VT_MODELS, None)}
  }
}

impl ::flatbuffers::Verifiable for SceneEntity<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("frame_id", Self::VT_FRAME_ID, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("id", Self::VT_ID, false)?
     .visit_field::<Duration>("lifetime", Self::VT_LIFETIME, false)?
     .visit_field::<bool>("frame_locked", Self::VT_FRAME_LOCKED, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<KeyValuePair>>>>("metadata", Self::VT_METADATA, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<ArrowPrimitive>>>>("arrows", Self::VT_ARROWS, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<CubePrimitive>>>>("cubes", Self::VT_CUBES, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<SpherePrimitive>>>>("spheres", Self::VT_SPHERES, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<CylinderPrimitive>>>>("cylinders", Self::VT_CYLINDERS, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<LinePrimitive>>>>("lines", Self::VT_LINES, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<TriangleListPrimitive>>>>("triangles", Self::VT_TRIANGLES, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<TextPrimitive>>>>("texts", Self::VT_TEXTS, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<ModelPrimitive>>>>("models", Self::VT_MODELS, false)?
     .finish();
    Ok(())
  }
}
pub struct SceneEntityArgs<'a> {
    pub timestamp: Option<&'a Time>,
    pub frame_id: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub id: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub lifetime: Option<&'a Duration>,
    pub frame_locked: bool,
    pub metadata: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<KeyValuePair<'a>>>>>,
    pub arrows: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<ArrowPrimitive<'a>>>>>,
    pub cubes: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<CubePrimitive<'a>>>>>,
    pub spheres: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<SpherePrimitive<'a>>>>>,
    pub cylinders: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<CylinderPrimitive<'a>>>>>,
    pub lines: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<LinePrimitive<'a>>>>>,
    pub triangles: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<TriangleListPrimitive<'a>>>>>,
    pub texts: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<TextPrimitive<'a>>>>>,
    pub models: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<ModelPrimitive<'a>>>>>,
}
impl<'a> Default for SceneEntityArgs<'a> {
  #[inline]
  fn default() -> Self {
    SceneEntityArgs {
      timestamp: None,
      frame_id: None,
      id: None,
      lifetime: None,
      frame_locked: false,
      metadata: None,
      arrows: None,
      cubes: None,
      spheres: None,
      cylinders: None,
      lines: None,
      triangles: None,
      texts: None,
      models: None,
    }
  }
}

pub struct SceneEntityBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> SceneEntityBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(SceneEntity::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn add_frame_id(&mut self, frame_id: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(SceneEntity::VT_FRAME_ID, frame_id);
  }
  #[inline]
  pub fn add_id(&mut self, id: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(SceneEntity::VT_ID, id);
  }
  #[inline]
  pub fn add_lifetime(&mut self, lifetime: &Duration) {
    self.fbb_.push_slot_always::<&Duration>(SceneEntity::VT_LIFETIME, lifetime);
  }
  #[inline]
  pub fn add_frame_locked(&mut self, frame_locked: bool) {
    self.fbb_.push_slot::<bool>(SceneEntity::VT_FRAME_LOCKED, frame_locked, false);
  }
  #[inline]
  pub fn add_metadata(&mut self, metadata: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<KeyValuePair<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(SceneEntity::VT_METADATA, metadata);
  }
  #[inline]
  pub fn add_arrows(&mut self, arrows: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<ArrowPrimitive<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(SceneEntity::VT_ARROWS, arrows);
  }
  #[inline]
  pub fn add_cubes(&mut self, cubes: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<CubePrimitive<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(SceneEntity::VT_CUBES, cubes);
  }
  #[inline]
  pub fn add_spheres(&mut self, spheres: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<SpherePrimitive<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(SceneEntity::VT_SPHERES, spheres);
  }
  #[inline]
  pub fn add_cylinders(&mut self, cylinders: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<CylinderPrimitive<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(SceneEntity::VT_CYLINDERS, cylinders);
  }
  #[inline]
  pub fn add_lines(&mut self, lines: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<LinePrimitive<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(SceneEntity::VT_LINES, lines);
  }
  #[inline]
  pub fn add_triangles(&mut self, triangles: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<TriangleListPrimitive<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(SceneEntity::VT_TRIANGLES, triangles);
  }
  #[inline]
  pub fn add_texts(&mut self, texts: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<TextPrimitive<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(SceneEntity::VT_TEXTS, texts);
  }
  #[inline]
  pub fn add_models(&mut self, models: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<ModelPrimitive<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(SceneEntity::VT_MODELS, models);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> SceneEntityBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    SceneEntityBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<SceneEntity<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for SceneEntity<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("SceneEntity");
      ds.field("timestamp", &self.timestamp());
      ds.field("frame_id", &self.frame_id());
      ds.field("id", &self.id());
      ds.field("lifetime", &self.lifetime());
      ds.field("frame_locked", &self.frame_locked());
      ds.field("metadata", &self.metadata());
      ds.field("arrows", &self.arrows());
      ds.field("cubes", &self.cubes());
      ds.field("spheres", &self.spheres());
      ds.field("cylinders", &self.cylinders());
      ds.field("lines", &self.lines());
      ds.field("triangles", &self.triangles());
      ds.field("texts", &self.texts());
      ds.field("models", &self.models());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `SceneEntity`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_scene_entity_unchecked`.
pub fn root_as_scene_entity(buf: &[u8]) -> Result<SceneEntity<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<SceneEntity>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `SceneEntity` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_scene_entity_unchecked`.
pub fn size_prefixed_root_as_scene_entity(buf: &[u8]) -> Result<SceneEntity<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<SceneEntity>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `SceneEntity` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_scene_entity_unchecked`.
pub fn root_as_scene_entity_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<SceneEntity<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<SceneEntity<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `SceneEntity` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_scene_entity_unchecked`.
pub fn size_prefixed_root_as_scene_entity_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<SceneEntity<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<SceneEntity<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a SceneEntity and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `SceneEntity`.
pub unsafe fn root_as_scene_entity_unchecked(buf: &[u8]) -> SceneEntity<'_> {
  unsafe { ::flatbuffers::root_unchecked::<SceneEntity>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed SceneEntity and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `SceneEntity`.
pub unsafe fn size_prefixed_root_as_scene_entity_unchecked(buf: &[u8]) -> SceneEntity<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<SceneEntity>(buf) }
}
#[inline]
pub fn finish_scene_entity_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<SceneEntity<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_scene_entity_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<SceneEntity<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from SceneUpdate_generated.rs =====



pub enum SceneUpdateOffset {}
#[derive(Copy, Clone, PartialEq)]

/// An update to the entities displayed in a 3D scene
pub struct SceneUpdate<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for SceneUpdate<'a> {
  type Inner = SceneUpdate<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> SceneUpdate<'a> {
  pub const VT_DELETIONS: ::flatbuffers::VOffsetT = 4;
  pub const VT_ENTITIES: ::flatbuffers::VOffsetT = 6;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    SceneUpdate { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args SceneUpdateArgs<'args>
  ) -> ::flatbuffers::WIPOffset<SceneUpdate<'bldr>> {
    let mut builder = SceneUpdateBuilder::new(_fbb);
    if let Some(x) = args.entities { builder.add_entities(x); }
    if let Some(x) = args.deletions { builder.add_deletions(x); }
    builder.finish()
  }


  /// Scene entities to delete
  #[inline]
  pub fn deletions(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<SceneEntityDeletion<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<SceneEntityDeletion>>>>(SceneUpdate::VT_DELETIONS, None)}
  }
  /// Scene entities to add or replace
  #[inline]
  pub fn entities(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<SceneEntity<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<SceneEntity>>>>(SceneUpdate::VT_ENTITIES, None)}
  }
}

impl ::flatbuffers::Verifiable for SceneUpdate<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<SceneEntityDeletion>>>>("deletions", Self::VT_DELETIONS, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<SceneEntity>>>>("entities", Self::VT_ENTITIES, false)?
     .finish();
    Ok(())
  }
}
pub struct SceneUpdateArgs<'a> {
    pub deletions: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<SceneEntityDeletion<'a>>>>>,
    pub entities: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<SceneEntity<'a>>>>>,
}
impl<'a> Default for SceneUpdateArgs<'a> {
  #[inline]
  fn default() -> Self {
    SceneUpdateArgs {
      deletions: None,
      entities: None,
    }
  }
}

pub struct SceneUpdateBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> SceneUpdateBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_deletions(&mut self, deletions: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<SceneEntityDeletion<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(SceneUpdate::VT_DELETIONS, deletions);
  }
  #[inline]
  pub fn add_entities(&mut self, entities: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<SceneEntity<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(SceneUpdate::VT_ENTITIES, entities);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> SceneUpdateBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    SceneUpdateBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<SceneUpdate<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for SceneUpdate<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("SceneUpdate");
      ds.field("deletions", &self.deletions());
      ds.field("entities", &self.entities());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `SceneUpdate`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_scene_update_unchecked`.
pub fn root_as_scene_update(buf: &[u8]) -> Result<SceneUpdate<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<SceneUpdate>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `SceneUpdate` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_scene_update_unchecked`.
pub fn size_prefixed_root_as_scene_update(buf: &[u8]) -> Result<SceneUpdate<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<SceneUpdate>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `SceneUpdate` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_scene_update_unchecked`.
pub fn root_as_scene_update_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<SceneUpdate<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<SceneUpdate<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `SceneUpdate` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_scene_update_unchecked`.
pub fn size_prefixed_root_as_scene_update_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<SceneUpdate<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<SceneUpdate<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a SceneUpdate and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `SceneUpdate`.
pub unsafe fn root_as_scene_update_unchecked(buf: &[u8]) -> SceneUpdate<'_> {
  unsafe { ::flatbuffers::root_unchecked::<SceneUpdate>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed SceneUpdate and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `SceneUpdate`.
pub unsafe fn size_prefixed_root_as_scene_update_unchecked(buf: &[u8]) -> SceneUpdate<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<SceneUpdate>(buf) }
}
#[inline]
pub fn finish_scene_update_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<SceneUpdate<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_scene_update_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<SceneUpdate<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from SpherePrimitive_generated.rs =====



pub enum SpherePrimitiveOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A primitive representing a sphere or ellipsoid
pub struct SpherePrimitive<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for SpherePrimitive<'a> {
  type Inner = SpherePrimitive<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> SpherePrimitive<'a> {
  pub const VT_POSE: ::flatbuffers::VOffsetT = 4;
  pub const VT_SIZE: ::flatbuffers::VOffsetT = 6;
  pub const VT_COLOR: ::flatbuffers::VOffsetT = 8;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    SpherePrimitive { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args SpherePrimitiveArgs<'args>
  ) -> ::flatbuffers::WIPOffset<SpherePrimitive<'bldr>> {
    let mut builder = SpherePrimitiveBuilder::new(_fbb);
    if let Some(x) = args.color { builder.add_color(x); }
    if let Some(x) = args.size { builder.add_size(x); }
    if let Some(x) = args.pose { builder.add_pose(x); }
    builder.finish()
  }


  /// Position of the center of the sphere and orientation of the sphere
  #[inline]
  pub fn pose(&self) -> Option<Pose<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Pose>>(SpherePrimitive::VT_POSE, None)}
  }
  /// Size (diameter) of the sphere along each axis
  #[inline]
  pub fn size(&self) -> Option<Vector3<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Vector3>>(SpherePrimitive::VT_SIZE, None)}
  }
  /// Color of the sphere
  #[inline]
  pub fn color(&self) -> Option<Color<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Color>>(SpherePrimitive::VT_COLOR, None)}
  }
}

impl ::flatbuffers::Verifiable for SpherePrimitive<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Pose>>("pose", Self::VT_POSE, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Vector3>>("size", Self::VT_SIZE, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Color>>("color", Self::VT_COLOR, false)?
     .finish();
    Ok(())
  }
}
pub struct SpherePrimitiveArgs<'a> {
    pub pose: Option<::flatbuffers::WIPOffset<Pose<'a>>>,
    pub size: Option<::flatbuffers::WIPOffset<Vector3<'a>>>,
    pub color: Option<::flatbuffers::WIPOffset<Color<'a>>>,
}
impl<'a> Default for SpherePrimitiveArgs<'a> {
  #[inline]
  fn default() -> Self {
    SpherePrimitiveArgs {
      pose: None,
      size: None,
      color: None,
    }
  }
}

pub struct SpherePrimitiveBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> SpherePrimitiveBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_pose(&mut self, pose: ::flatbuffers::WIPOffset<Pose<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Pose>>(SpherePrimitive::VT_POSE, pose);
  }
  #[inline]
  pub fn add_size(&mut self, size: ::flatbuffers::WIPOffset<Vector3<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Vector3>>(SpherePrimitive::VT_SIZE, size);
  }
  #[inline]
  pub fn add_color(&mut self, color: ::flatbuffers::WIPOffset<Color<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Color>>(SpherePrimitive::VT_COLOR, color);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> SpherePrimitiveBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    SpherePrimitiveBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<SpherePrimitive<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for SpherePrimitive<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("SpherePrimitive");
      ds.field("pose", &self.pose());
      ds.field("size", &self.size());
      ds.field("color", &self.color());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `SpherePrimitive`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_sphere_primitive_unchecked`.
pub fn root_as_sphere_primitive(buf: &[u8]) -> Result<SpherePrimitive<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<SpherePrimitive>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `SpherePrimitive` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_sphere_primitive_unchecked`.
pub fn size_prefixed_root_as_sphere_primitive(buf: &[u8]) -> Result<SpherePrimitive<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<SpherePrimitive>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `SpherePrimitive` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_sphere_primitive_unchecked`.
pub fn root_as_sphere_primitive_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<SpherePrimitive<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<SpherePrimitive<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `SpherePrimitive` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_sphere_primitive_unchecked`.
pub fn size_prefixed_root_as_sphere_primitive_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<SpherePrimitive<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<SpherePrimitive<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a SpherePrimitive and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `SpherePrimitive`.
pub unsafe fn root_as_sphere_primitive_unchecked(buf: &[u8]) -> SpherePrimitive<'_> {
  unsafe { ::flatbuffers::root_unchecked::<SpherePrimitive>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed SpherePrimitive and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `SpherePrimitive`.
pub unsafe fn size_prefixed_root_as_sphere_primitive_unchecked(buf: &[u8]) -> SpherePrimitive<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<SpherePrimitive>(buf) }
}
#[inline]
pub fn finish_sphere_primitive_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<SpherePrimitive<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_sphere_primitive_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<SpherePrimitive<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from TextAnnotation_generated.rs =====



pub enum TextAnnotationOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A text label on a 2D image
pub struct TextAnnotation<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for TextAnnotation<'a> {
  type Inner = TextAnnotation<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> TextAnnotation<'a> {
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
  pub const VT_POSITION: ::flatbuffers::VOffsetT = 6;
  pub const VT_TEXT: ::flatbuffers::VOffsetT = 8;
  pub const VT_FONT_SIZE: ::flatbuffers::VOffsetT = 10;
  pub const VT_TEXT_COLOR: ::flatbuffers::VOffsetT = 12;
  pub const VT_BACKGROUND_COLOR: ::flatbuffers::VOffsetT = 14;
  pub const VT_METADATA: ::flatbuffers::VOffsetT = 16;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    TextAnnotation { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args TextAnnotationArgs<'args>
  ) -> ::flatbuffers::WIPOffset<TextAnnotation<'bldr>> {
    let mut builder = TextAnnotationBuilder::new(_fbb);
    builder.add_font_size(args.font_size);
    if let Some(x) = args.metadata { builder.add_metadata(x); }
    if let Some(x) = args.background_color { builder.add_background_color(x); }
    if let Some(x) = args.text_color { builder.add_text_color(x); }
    if let Some(x) = args.text { builder.add_text(x); }
    if let Some(x) = args.position { builder.add_position(x); }
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    builder.finish()
  }


  /// Timestamp of annotation
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(TextAnnotation::VT_TIMESTAMP, None)}
  }
  /// Bottom-left origin of the text label in 2D image coordinates (pixels).
  /// The coordinate uses the top-left corner of the top-left pixel of the image as the origin.
  #[inline]
  pub fn position(&self) -> Option<Point2<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Point2>>(TextAnnotation::VT_POSITION, None)}
  }
  /// Text to display
  #[inline]
  pub fn text(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(TextAnnotation::VT_TEXT, None)}
  }
  /// Font size in pixels
  #[inline]
  pub fn font_size(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(TextAnnotation::VT_FONT_SIZE, Some(12.0)).unwrap()}
  }
  /// Text color
  #[inline]
  pub fn text_color(&self) -> Option<Color<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Color>>(TextAnnotation::VT_TEXT_COLOR, None)}
  }
  /// Background fill color
  #[inline]
  pub fn background_color(&self) -> Option<Color<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Color>>(TextAnnotation::VT_BACKGROUND_COLOR, None)}
  }
  /// Additional user-provided metadata associated with this annotation. Keys must be unique.
  #[inline]
  pub fn metadata(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<KeyValuePair<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<KeyValuePair>>>>(TextAnnotation::VT_METADATA, None)}
  }
}

impl ::flatbuffers::Verifiable for TextAnnotation<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Point2>>("position", Self::VT_POSITION, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("text", Self::VT_TEXT, false)?
     .visit_field::<f64>("font_size", Self::VT_FONT_SIZE, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Color>>("text_color", Self::VT_TEXT_COLOR, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Color>>("background_color", Self::VT_BACKGROUND_COLOR, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<KeyValuePair>>>>("metadata", Self::VT_METADATA, false)?
     .finish();
    Ok(())
  }
}
pub struct TextAnnotationArgs<'a> {
    pub timestamp: Option<&'a Time>,
    pub position: Option<::flatbuffers::WIPOffset<Point2<'a>>>,
    pub text: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub font_size: f64,
    pub text_color: Option<::flatbuffers::WIPOffset<Color<'a>>>,
    pub background_color: Option<::flatbuffers::WIPOffset<Color<'a>>>,
    pub metadata: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<KeyValuePair<'a>>>>>,
}
impl<'a> Default for TextAnnotationArgs<'a> {
  #[inline]
  fn default() -> Self {
    TextAnnotationArgs {
      timestamp: None,
      position: None,
      text: None,
      font_size: 12.0,
      text_color: None,
      background_color: None,
      metadata: None,
    }
  }
}

pub struct TextAnnotationBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> TextAnnotationBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(TextAnnotation::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn add_position(&mut self, position: ::flatbuffers::WIPOffset<Point2<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Point2>>(TextAnnotation::VT_POSITION, position);
  }
  #[inline]
  pub fn add_text(&mut self, text: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(TextAnnotation::VT_TEXT, text);
  }
  #[inline]
  pub fn add_font_size(&mut self, font_size: f64) {
    self.fbb_.push_slot::<f64>(TextAnnotation::VT_FONT_SIZE, font_size, 12.0);
  }
  #[inline]
  pub fn add_text_color(&mut self, text_color: ::flatbuffers::WIPOffset<Color<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Color>>(TextAnnotation::VT_TEXT_COLOR, text_color);
  }
  #[inline]
  pub fn add_background_color(&mut self, background_color: ::flatbuffers::WIPOffset<Color<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Color>>(TextAnnotation::VT_BACKGROUND_COLOR, background_color);
  }
  #[inline]
  pub fn add_metadata(&mut self, metadata: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<KeyValuePair<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(TextAnnotation::VT_METADATA, metadata);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> TextAnnotationBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    TextAnnotationBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<TextAnnotation<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for TextAnnotation<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("TextAnnotation");
      ds.field("timestamp", &self.timestamp());
      ds.field("position", &self.position());
      ds.field("text", &self.text());
      ds.field("font_size", &self.font_size());
      ds.field("text_color", &self.text_color());
      ds.field("background_color", &self.background_color());
      ds.field("metadata", &self.metadata());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `TextAnnotation`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_text_annotation_unchecked`.
pub fn root_as_text_annotation(buf: &[u8]) -> Result<TextAnnotation<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<TextAnnotation>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `TextAnnotation` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_text_annotation_unchecked`.
pub fn size_prefixed_root_as_text_annotation(buf: &[u8]) -> Result<TextAnnotation<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<TextAnnotation>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `TextAnnotation` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_text_annotation_unchecked`.
pub fn root_as_text_annotation_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<TextAnnotation<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<TextAnnotation<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `TextAnnotation` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_text_annotation_unchecked`.
pub fn size_prefixed_root_as_text_annotation_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<TextAnnotation<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<TextAnnotation<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a TextAnnotation and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `TextAnnotation`.
pub unsafe fn root_as_text_annotation_unchecked(buf: &[u8]) -> TextAnnotation<'_> {
  unsafe { ::flatbuffers::root_unchecked::<TextAnnotation>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed TextAnnotation and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `TextAnnotation`.
pub unsafe fn size_prefixed_root_as_text_annotation_unchecked(buf: &[u8]) -> TextAnnotation<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<TextAnnotation>(buf) }
}
#[inline]
pub fn finish_text_annotation_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<TextAnnotation<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_text_annotation_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<TextAnnotation<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from TextPrimitive_generated.rs =====



pub enum TextPrimitiveOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A primitive representing a text label
pub struct TextPrimitive<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for TextPrimitive<'a> {
  type Inner = TextPrimitive<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> TextPrimitive<'a> {
  pub const VT_POSE: ::flatbuffers::VOffsetT = 4;
  pub const VT_BILLBOARD: ::flatbuffers::VOffsetT = 6;
  pub const VT_FONT_SIZE: ::flatbuffers::VOffsetT = 8;
  pub const VT_SCALE_INVARIANT: ::flatbuffers::VOffsetT = 10;
  pub const VT_COLOR: ::flatbuffers::VOffsetT = 12;
  pub const VT_TEXT: ::flatbuffers::VOffsetT = 14;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    TextPrimitive { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args TextPrimitiveArgs<'args>
  ) -> ::flatbuffers::WIPOffset<TextPrimitive<'bldr>> {
    let mut builder = TextPrimitiveBuilder::new(_fbb);
    builder.add_font_size(args.font_size);
    if let Some(x) = args.text { builder.add_text(x); }
    if let Some(x) = args.color { builder.add_color(x); }
    if let Some(x) = args.pose { builder.add_pose(x); }
    builder.add_scale_invariant(args.scale_invariant);
    builder.add_billboard(args.billboard);
    builder.finish()
  }


  /// Position of the center of the text box and orientation of the text. Identity orientation means the text is oriented in the xy-plane and flows from -x to +x.
  #[inline]
  pub fn pose(&self) -> Option<Pose<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Pose>>(TextPrimitive::VT_POSE, None)}
  }
  /// Whether the text should respect `pose.orientation` (false) or always face the camera (true)
  #[inline]
  pub fn billboard(&self) -> bool {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<bool>(TextPrimitive::VT_BILLBOARD, Some(false)).unwrap()}
  }
  /// Font size (height of one line of text)
  #[inline]
  pub fn font_size(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(TextPrimitive::VT_FONT_SIZE, Some(0.0)).unwrap()}
  }
  /// Indicates whether `font_size` is a fixed size in screen pixels (true), or specified in world coordinates and scales with distance from the camera (false)
  #[inline]
  pub fn scale_invariant(&self) -> bool {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<bool>(TextPrimitive::VT_SCALE_INVARIANT, Some(false)).unwrap()}
  }
  /// Color of the text
  #[inline]
  pub fn color(&self) -> Option<Color<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Color>>(TextPrimitive::VT_COLOR, None)}
  }
  /// Text
  #[inline]
  pub fn text(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(TextPrimitive::VT_TEXT, None)}
  }
}

impl ::flatbuffers::Verifiable for TextPrimitive<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Pose>>("pose", Self::VT_POSE, false)?
     .visit_field::<bool>("billboard", Self::VT_BILLBOARD, false)?
     .visit_field::<f64>("font_size", Self::VT_FONT_SIZE, false)?
     .visit_field::<bool>("scale_invariant", Self::VT_SCALE_INVARIANT, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Color>>("color", Self::VT_COLOR, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("text", Self::VT_TEXT, false)?
     .finish();
    Ok(())
  }
}
pub struct TextPrimitiveArgs<'a> {
    pub pose: Option<::flatbuffers::WIPOffset<Pose<'a>>>,
    pub billboard: bool,
    pub font_size: f64,
    pub scale_invariant: bool,
    pub color: Option<::flatbuffers::WIPOffset<Color<'a>>>,
    pub text: Option<::flatbuffers::WIPOffset<&'a str>>,
}
impl<'a> Default for TextPrimitiveArgs<'a> {
  #[inline]
  fn default() -> Self {
    TextPrimitiveArgs {
      pose: None,
      billboard: false,
      font_size: 0.0,
      scale_invariant: false,
      color: None,
      text: None,
    }
  }
}

pub struct TextPrimitiveBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> TextPrimitiveBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_pose(&mut self, pose: ::flatbuffers::WIPOffset<Pose<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Pose>>(TextPrimitive::VT_POSE, pose);
  }
  #[inline]
  pub fn add_billboard(&mut self, billboard: bool) {
    self.fbb_.push_slot::<bool>(TextPrimitive::VT_BILLBOARD, billboard, false);
  }
  #[inline]
  pub fn add_font_size(&mut self, font_size: f64) {
    self.fbb_.push_slot::<f64>(TextPrimitive::VT_FONT_SIZE, font_size, 0.0);
  }
  #[inline]
  pub fn add_scale_invariant(&mut self, scale_invariant: bool) {
    self.fbb_.push_slot::<bool>(TextPrimitive::VT_SCALE_INVARIANT, scale_invariant, false);
  }
  #[inline]
  pub fn add_color(&mut self, color: ::flatbuffers::WIPOffset<Color<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Color>>(TextPrimitive::VT_COLOR, color);
  }
  #[inline]
  pub fn add_text(&mut self, text: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(TextPrimitive::VT_TEXT, text);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> TextPrimitiveBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    TextPrimitiveBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<TextPrimitive<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for TextPrimitive<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("TextPrimitive");
      ds.field("pose", &self.pose());
      ds.field("billboard", &self.billboard());
      ds.field("font_size", &self.font_size());
      ds.field("scale_invariant", &self.scale_invariant());
      ds.field("color", &self.color());
      ds.field("text", &self.text());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `TextPrimitive`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_text_primitive_unchecked`.
pub fn root_as_text_primitive(buf: &[u8]) -> Result<TextPrimitive<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<TextPrimitive>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `TextPrimitive` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_text_primitive_unchecked`.
pub fn size_prefixed_root_as_text_primitive(buf: &[u8]) -> Result<TextPrimitive<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<TextPrimitive>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `TextPrimitive` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_text_primitive_unchecked`.
pub fn root_as_text_primitive_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<TextPrimitive<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<TextPrimitive<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `TextPrimitive` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_text_primitive_unchecked`.
pub fn size_prefixed_root_as_text_primitive_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<TextPrimitive<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<TextPrimitive<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a TextPrimitive and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `TextPrimitive`.
pub unsafe fn root_as_text_primitive_unchecked(buf: &[u8]) -> TextPrimitive<'_> {
  unsafe { ::flatbuffers::root_unchecked::<TextPrimitive>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed TextPrimitive and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `TextPrimitive`.
pub unsafe fn size_prefixed_root_as_text_primitive_unchecked(buf: &[u8]) -> TextPrimitive<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<TextPrimitive>(buf) }
}
#[inline]
pub fn finish_text_primitive_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<TextPrimitive<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_text_primitive_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<TextPrimitive<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from Time_generated.rs =====



// struct Time, aligned to 4
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq)]
pub struct Time(pub [u8; 8]);
impl Default for Time { 
  fn default() -> Self { 
    Self([0; 8])
  }
}
impl ::core::fmt::Debug for Time {
  fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
    f.debug_struct("Time")
      .field("sec", &self.sec())
      .field("nsec", &self.nsec())
      .finish()
  }
}

impl ::flatbuffers::SimpleToVerifyInSlice for Time {}
impl<'a> ::flatbuffers::Follow<'a> for Time {
  type Inner = &'a Time;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    unsafe { <&'a Time>::follow(buf, loc) }
  }
}
impl<'a> ::flatbuffers::Follow<'a> for &'a Time {
  type Inner = &'a Time;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    unsafe { ::flatbuffers::follow_cast_ref::<Time>(buf, loc) }
  }
}
impl<'b> ::flatbuffers::Push for Time {
    type Output = Time;
    #[inline]
    unsafe fn push(&self, dst: &mut [u8], _written_len: usize) {
        let src = unsafe { ::core::slice::from_raw_parts(self as *const Time as *const u8, <Self as ::flatbuffers::Push>::size()) };
        dst.copy_from_slice(src);
    }
    #[inline]
    fn alignment() -> ::flatbuffers::PushAlignment {
        ::flatbuffers::PushAlignment::new(4)
    }
}

impl<'a> ::flatbuffers::Verifiable for Time {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.in_buffer::<Self>(pos)
  }
}

impl<'a> Time {
  #[allow(clippy::too_many_arguments)]
  pub fn new(
    sec: u32,
    nsec: u32,
  ) -> Self {
    let mut s = Self([0; 8]);
    s.set_sec(sec);
    s.set_nsec(nsec);
    s
  }

  /// Represents seconds of UTC time since Unix epoch 1970-01-01T00:00:00Z
  pub fn sec(&self) -> u32 {
    let mut mem = ::core::mem::MaybeUninit::<<u32 as ::flatbuffers::EndianScalar>::Scalar>::uninit();
    // Safety:
    // Created from a valid Table for this object
    // Which contains a valid value in this slot
    ::flatbuffers::EndianScalar::from_little_endian(unsafe {
      ::core::ptr::copy_nonoverlapping(
        self.0[0..].as_ptr(),
        mem.as_mut_ptr() as *mut u8,
        ::core::mem::size_of::<<u32 as ::flatbuffers::EndianScalar>::Scalar>(),
      );
      mem.assume_init()
    })
  }

  pub fn set_sec(&mut self, x: u32) {
    let x_le = ::flatbuffers::EndianScalar::to_little_endian(x);
    // Safety:
    // Created from a valid Table for this object
    // Which contains a valid value in this slot
    unsafe {
      ::core::ptr::copy_nonoverlapping(
        &x_le as *const _ as *const u8,
        self.0[0..].as_mut_ptr(),
        ::core::mem::size_of::<<u32 as ::flatbuffers::EndianScalar>::Scalar>(),
      );
    }
  }

  /// Nano-second fractions from 0 to 999,999,999 inclusive
  pub fn nsec(&self) -> u32 {
    let mut mem = ::core::mem::MaybeUninit::<<u32 as ::flatbuffers::EndianScalar>::Scalar>::uninit();
    // Safety:
    // Created from a valid Table for this object
    // Which contains a valid value in this slot
    ::flatbuffers::EndianScalar::from_little_endian(unsafe {
      ::core::ptr::copy_nonoverlapping(
        self.0[4..].as_ptr(),
        mem.as_mut_ptr() as *mut u8,
        ::core::mem::size_of::<<u32 as ::flatbuffers::EndianScalar>::Scalar>(),
      );
      mem.assume_init()
    })
  }

  pub fn set_nsec(&mut self, x: u32) {
    let x_le = ::flatbuffers::EndianScalar::to_little_endian(x);
    // Safety:
    // Created from a valid Table for this object
    // Which contains a valid value in this slot
    unsafe {
      ::core::ptr::copy_nonoverlapping(
        &x_le as *const _ as *const u8,
        self.0[4..].as_mut_ptr(),
        ::core::mem::size_of::<<u32 as ::flatbuffers::EndianScalar>::Scalar>(),
      );
    }
  }

}


// ===== from TriangleListPrimitive_generated.rs =====



pub enum TriangleListPrimitiveOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A primitive representing a set of triangles or a surface tiled by triangles
pub struct TriangleListPrimitive<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for TriangleListPrimitive<'a> {
  type Inner = TriangleListPrimitive<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> TriangleListPrimitive<'a> {
  pub const VT_POSE: ::flatbuffers::VOffsetT = 4;
  pub const VT_POINTS: ::flatbuffers::VOffsetT = 6;
  pub const VT_COLOR: ::flatbuffers::VOffsetT = 8;
  pub const VT_COLORS: ::flatbuffers::VOffsetT = 10;
  pub const VT_INDICES: ::flatbuffers::VOffsetT = 12;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    TriangleListPrimitive { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args TriangleListPrimitiveArgs<'args>
  ) -> ::flatbuffers::WIPOffset<TriangleListPrimitive<'bldr>> {
    let mut builder = TriangleListPrimitiveBuilder::new(_fbb);
    if let Some(x) = args.indices { builder.add_indices(x); }
    if let Some(x) = args.colors { builder.add_colors(x); }
    if let Some(x) = args.color { builder.add_color(x); }
    if let Some(x) = args.points { builder.add_points(x); }
    if let Some(x) = args.pose { builder.add_pose(x); }
    builder.finish()
  }


  /// Origin of triangles relative to reference frame
  #[inline]
  pub fn pose(&self) -> Option<Pose<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Pose>>(TriangleListPrimitive::VT_POSE, None)}
  }
  /// Vertices to use for triangles, interpreted as a list of triples (0-1-2, 3-4-5, ...)
  #[inline]
  pub fn points(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<Point3<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<Point3>>>>(TriangleListPrimitive::VT_POINTS, None)}
  }
  /// Solid color to use for the whole shape. Ignored if `colors` is non-empty.
  #[inline]
  pub fn color(&self) -> Option<Color<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Color>>(TriangleListPrimitive::VT_COLOR, None)}
  }
  /// Per-vertex colors (if specified, must have the same length as `points`).
  #[inline]
  pub fn colors(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<Color<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<Color>>>>(TriangleListPrimitive::VT_COLORS, None)}
  }
  /// Indices into the `points` and `colors` attribute arrays, which can be used to avoid duplicating attribute data.
  /// 
  /// If omitted or empty, indexing will not be used. This default behavior is equivalent to specifying [0, 1, ..., N-1] for the indices (where N is the number of `points` provided).
  #[inline]
  pub fn indices(&self) -> Option<::flatbuffers::Vector<'a, u32>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, u32>>>(TriangleListPrimitive::VT_INDICES, None)}
  }
}

impl ::flatbuffers::Verifiable for TriangleListPrimitive<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Pose>>("pose", Self::VT_POSE, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<Point3>>>>("points", Self::VT_POINTS, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Color>>("color", Self::VT_COLOR, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<Color>>>>("colors", Self::VT_COLORS, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, u32>>>("indices", Self::VT_INDICES, false)?
     .finish();
    Ok(())
  }
}
pub struct TriangleListPrimitiveArgs<'a> {
    pub pose: Option<::flatbuffers::WIPOffset<Pose<'a>>>,
    pub points: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<Point3<'a>>>>>,
    pub color: Option<::flatbuffers::WIPOffset<Color<'a>>>,
    pub colors: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<Color<'a>>>>>,
    pub indices: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, u32>>>,
}
impl<'a> Default for TriangleListPrimitiveArgs<'a> {
  #[inline]
  fn default() -> Self {
    TriangleListPrimitiveArgs {
      pose: None,
      points: None,
      color: None,
      colors: None,
      indices: None,
    }
  }
}

pub struct TriangleListPrimitiveBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> TriangleListPrimitiveBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_pose(&mut self, pose: ::flatbuffers::WIPOffset<Pose<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Pose>>(TriangleListPrimitive::VT_POSE, pose);
  }
  #[inline]
  pub fn add_points(&mut self, points: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<Point3<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(TriangleListPrimitive::VT_POINTS, points);
  }
  #[inline]
  pub fn add_color(&mut self, color: ::flatbuffers::WIPOffset<Color<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Color>>(TriangleListPrimitive::VT_COLOR, color);
  }
  #[inline]
  pub fn add_colors(&mut self, colors: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<Color<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(TriangleListPrimitive::VT_COLORS, colors);
  }
  #[inline]
  pub fn add_indices(&mut self, indices: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , u32>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(TriangleListPrimitive::VT_INDICES, indices);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> TriangleListPrimitiveBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    TriangleListPrimitiveBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<TriangleListPrimitive<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for TriangleListPrimitive<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("TriangleListPrimitive");
      ds.field("pose", &self.pose());
      ds.field("points", &self.points());
      ds.field("color", &self.color());
      ds.field("colors", &self.colors());
      ds.field("indices", &self.indices());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `TriangleListPrimitive`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_triangle_list_primitive_unchecked`.
pub fn root_as_triangle_list_primitive(buf: &[u8]) -> Result<TriangleListPrimitive<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<TriangleListPrimitive>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `TriangleListPrimitive` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_triangle_list_primitive_unchecked`.
pub fn size_prefixed_root_as_triangle_list_primitive(buf: &[u8]) -> Result<TriangleListPrimitive<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<TriangleListPrimitive>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `TriangleListPrimitive` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_triangle_list_primitive_unchecked`.
pub fn root_as_triangle_list_primitive_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<TriangleListPrimitive<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<TriangleListPrimitive<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `TriangleListPrimitive` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_triangle_list_primitive_unchecked`.
pub fn size_prefixed_root_as_triangle_list_primitive_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<TriangleListPrimitive<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<TriangleListPrimitive<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a TriangleListPrimitive and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `TriangleListPrimitive`.
pub unsafe fn root_as_triangle_list_primitive_unchecked(buf: &[u8]) -> TriangleListPrimitive<'_> {
  unsafe { ::flatbuffers::root_unchecked::<TriangleListPrimitive>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed TriangleListPrimitive and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `TriangleListPrimitive`.
pub unsafe fn size_prefixed_root_as_triangle_list_primitive_unchecked(buf: &[u8]) -> TriangleListPrimitive<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<TriangleListPrimitive>(buf) }
}
#[inline]
pub fn finish_triangle_list_primitive_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<TriangleListPrimitive<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_triangle_list_primitive_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<TriangleListPrimitive<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from Vector2_generated.rs =====



pub enum Vector2Offset {}
#[derive(Copy, Clone, PartialEq)]

/// A vector in 2D space that represents a direction only
pub struct Vector2<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for Vector2<'a> {
  type Inner = Vector2<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> Vector2<'a> {
  pub const VT_X: ::flatbuffers::VOffsetT = 4;
  pub const VT_Y: ::flatbuffers::VOffsetT = 6;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    Vector2 { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args Vector2Args
  ) -> ::flatbuffers::WIPOffset<Vector2<'bldr>> {
    let mut builder = Vector2Builder::new(_fbb);
    builder.add_y(args.y);
    builder.add_x(args.x);
    builder.finish()
  }


  /// x component
  #[inline]
  pub fn x(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(Vector2::VT_X, Some(0.0)).unwrap()}
  }
  /// y component
  #[inline]
  pub fn y(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(Vector2::VT_Y, Some(0.0)).unwrap()}
  }
}

impl ::flatbuffers::Verifiable for Vector2<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<f64>("x", Self::VT_X, false)?
     .visit_field::<f64>("y", Self::VT_Y, false)?
     .finish();
    Ok(())
  }
}
pub struct Vector2Args {
    pub x: f64,
    pub y: f64,
}
impl<'a> Default for Vector2Args {
  #[inline]
  fn default() -> Self {
    Vector2Args {
      x: 0.0,
      y: 0.0,
    }
  }
}

pub struct Vector2Builder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> Vector2Builder<'a, 'b, A> {
  #[inline]
  pub fn add_x(&mut self, x: f64) {
    self.fbb_.push_slot::<f64>(Vector2::VT_X, x, 0.0);
  }
  #[inline]
  pub fn add_y(&mut self, y: f64) {
    self.fbb_.push_slot::<f64>(Vector2::VT_Y, y, 0.0);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> Vector2Builder<'a, 'b, A> {
    let start = _fbb.start_table();
    Vector2Builder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<Vector2<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for Vector2<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("Vector2");
      ds.field("x", &self.x());
      ds.field("y", &self.y());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `Vector2`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_vector_2_unchecked`.
pub fn root_as_vector_2(buf: &[u8]) -> Result<Vector2<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<Vector2>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `Vector2` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_vector_2_unchecked`.
pub fn size_prefixed_root_as_vector_2(buf: &[u8]) -> Result<Vector2<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<Vector2>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `Vector2` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_vector_2_unchecked`.
pub fn root_as_vector_2_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<Vector2<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<Vector2<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `Vector2` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_vector_2_unchecked`.
pub fn size_prefixed_root_as_vector_2_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<Vector2<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<Vector2<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a Vector2 and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `Vector2`.
pub unsafe fn root_as_vector_2_unchecked(buf: &[u8]) -> Vector2<'_> {
  unsafe { ::flatbuffers::root_unchecked::<Vector2>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed Vector2 and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `Vector2`.
pub unsafe fn size_prefixed_root_as_vector_2_unchecked(buf: &[u8]) -> Vector2<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<Vector2>(buf) }
}
#[inline]
pub fn finish_vector_2_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<Vector2<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_vector_2_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<Vector2<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from Vector3_generated.rs =====



pub enum Vector3Offset {}
#[derive(Copy, Clone, PartialEq)]

/// A vector in 3D space that represents a direction only
pub struct Vector3<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for Vector3<'a> {
  type Inner = Vector3<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> Vector3<'a> {
  pub const VT_X: ::flatbuffers::VOffsetT = 4;
  pub const VT_Y: ::flatbuffers::VOffsetT = 6;
  pub const VT_Z: ::flatbuffers::VOffsetT = 8;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    Vector3 { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args Vector3Args
  ) -> ::flatbuffers::WIPOffset<Vector3<'bldr>> {
    let mut builder = Vector3Builder::new(_fbb);
    builder.add_z(args.z);
    builder.add_y(args.y);
    builder.add_x(args.x);
    builder.finish()
  }


  /// x component
  #[inline]
  pub fn x(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(Vector3::VT_X, Some(0.0)).unwrap()}
  }
  /// y component
  #[inline]
  pub fn y(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(Vector3::VT_Y, Some(0.0)).unwrap()}
  }
  /// z component
  #[inline]
  pub fn z(&self) -> f64 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<f64>(Vector3::VT_Z, Some(0.0)).unwrap()}
  }
}

impl ::flatbuffers::Verifiable for Vector3<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<f64>("x", Self::VT_X, false)?
     .visit_field::<f64>("y", Self::VT_Y, false)?
     .visit_field::<f64>("z", Self::VT_Z, false)?
     .finish();
    Ok(())
  }
}
pub struct Vector3Args {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}
impl<'a> Default for Vector3Args {
  #[inline]
  fn default() -> Self {
    Vector3Args {
      x: 0.0,
      y: 0.0,
      z: 0.0,
    }
  }
}

pub struct Vector3Builder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> Vector3Builder<'a, 'b, A> {
  #[inline]
  pub fn add_x(&mut self, x: f64) {
    self.fbb_.push_slot::<f64>(Vector3::VT_X, x, 0.0);
  }
  #[inline]
  pub fn add_y(&mut self, y: f64) {
    self.fbb_.push_slot::<f64>(Vector3::VT_Y, y, 0.0);
  }
  #[inline]
  pub fn add_z(&mut self, z: f64) {
    self.fbb_.push_slot::<f64>(Vector3::VT_Z, z, 0.0);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> Vector3Builder<'a, 'b, A> {
    let start = _fbb.start_table();
    Vector3Builder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<Vector3<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for Vector3<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("Vector3");
      ds.field("x", &self.x());
      ds.field("y", &self.y());
      ds.field("z", &self.z());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `Vector3`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_vector_3_unchecked`.
pub fn root_as_vector_3(buf: &[u8]) -> Result<Vector3<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<Vector3>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `Vector3` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_vector_3_unchecked`.
pub fn size_prefixed_root_as_vector_3(buf: &[u8]) -> Result<Vector3<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<Vector3>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `Vector3` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_vector_3_unchecked`.
pub fn root_as_vector_3_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<Vector3<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<Vector3<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `Vector3` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_vector_3_unchecked`.
pub fn size_prefixed_root_as_vector_3_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<Vector3<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<Vector3<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a Vector3 and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `Vector3`.
pub unsafe fn root_as_vector_3_unchecked(buf: &[u8]) -> Vector3<'_> {
  unsafe { ::flatbuffers::root_unchecked::<Vector3>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed Vector3 and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `Vector3`.
pub unsafe fn size_prefixed_root_as_vector_3_unchecked(buf: &[u8]) -> Vector3<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<Vector3>(buf) }
}
#[inline]
pub fn finish_vector_3_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<Vector3<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_vector_3_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<Vector3<'a>>) {
  fbb.finish_size_prefixed(root, None);
}

// ===== from VoxelGrid_generated.rs =====



pub enum VoxelGridOffset {}
#[derive(Copy, Clone, PartialEq)]

/// A 3D grid of data
pub struct VoxelGrid<'a> {
  pub _tab: ::flatbuffers::Table<'a>,
}

impl<'a> ::flatbuffers::Follow<'a> for VoxelGrid<'a> {
  type Inner = VoxelGrid<'a>;
  #[inline]
  unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
    Self { _tab: unsafe { ::flatbuffers::Table::new(buf, loc) } }
  }
}

impl<'a> VoxelGrid<'a> {
  pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
  pub const VT_FRAME_ID: ::flatbuffers::VOffsetT = 6;
  pub const VT_POSE: ::flatbuffers::VOffsetT = 8;
  pub const VT_ROW_COUNT: ::flatbuffers::VOffsetT = 10;
  pub const VT_COLUMN_COUNT: ::flatbuffers::VOffsetT = 12;
  pub const VT_CELL_SIZE: ::flatbuffers::VOffsetT = 14;
  pub const VT_SLICE_STRIDE: ::flatbuffers::VOffsetT = 16;
  pub const VT_ROW_STRIDE: ::flatbuffers::VOffsetT = 18;
  pub const VT_CELL_STRIDE: ::flatbuffers::VOffsetT = 20;
  pub const VT_FIELDS: ::flatbuffers::VOffsetT = 22;
  pub const VT_DATA: ::flatbuffers::VOffsetT = 24;

  #[inline]
  pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
    VoxelGrid { _tab: table }
  }
  #[allow(unused_mut)]
  pub fn create<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr, A: ::flatbuffers::Allocator + 'bldr>(
    _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
    args: &'args VoxelGridArgs<'args>
  ) -> ::flatbuffers::WIPOffset<VoxelGrid<'bldr>> {
    let mut builder = VoxelGridBuilder::new(_fbb);
    if let Some(x) = args.data { builder.add_data(x); }
    if let Some(x) = args.fields { builder.add_fields(x); }
    builder.add_cell_stride(args.cell_stride);
    builder.add_row_stride(args.row_stride);
    builder.add_slice_stride(args.slice_stride);
    if let Some(x) = args.cell_size { builder.add_cell_size(x); }
    builder.add_column_count(args.column_count);
    builder.add_row_count(args.row_count);
    if let Some(x) = args.pose { builder.add_pose(x); }
    if let Some(x) = args.frame_id { builder.add_frame_id(x); }
    if let Some(x) = args.timestamp { builder.add_timestamp(x); }
    builder.finish()
  }


  /// Timestamp of grid
  #[inline]
  pub fn timestamp(&self) -> Option<&'a Time> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<Time>(VoxelGrid::VT_TIMESTAMP, None)}
  }
  /// Frame of reference
  #[inline]
  pub fn frame_id(&self) -> Option<&'a str> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<&str>>(VoxelGrid::VT_FRAME_ID, None)}
  }
  /// Origin of the grid’s lower-front-left corner in the reference frame. The grid’s pose is defined relative to this corner, so an untransformed grid with an identity orientation has this corner at the origin.
  #[inline]
  pub fn pose(&self) -> Option<Pose<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Pose>>(VoxelGrid::VT_POSE, None)}
  }
  /// Number of grid rows
  #[inline]
  pub fn row_count(&self) -> u32 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<u32>(VoxelGrid::VT_ROW_COUNT, Some(0)).unwrap()}
  }
  /// Number of grid columns
  #[inline]
  pub fn column_count(&self) -> u32 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<u32>(VoxelGrid::VT_COLUMN_COUNT, Some(0)).unwrap()}
  }
  /// Size of single grid cell along x, y, and z axes, relative to `pose`
  #[inline]
  pub fn cell_size(&self) -> Option<Vector3<'a>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<Vector3>>(VoxelGrid::VT_CELL_SIZE, None)}
  }
  /// Number of bytes between depth slices in `data`
  #[inline]
  pub fn slice_stride(&self) -> u32 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<u32>(VoxelGrid::VT_SLICE_STRIDE, Some(0)).unwrap()}
  }
  /// Number of bytes between rows in `data`
  #[inline]
  pub fn row_stride(&self) -> u32 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<u32>(VoxelGrid::VT_ROW_STRIDE, Some(0)).unwrap()}
  }
  /// Number of bytes between cells within a row in `data`
  #[inline]
  pub fn cell_stride(&self) -> u32 {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<u32>(VoxelGrid::VT_CELL_STRIDE, Some(0)).unwrap()}
  }
  /// Fields in `data`. `red`, `green`, `blue`, and `alpha` are optional for customizing the grid's color.
  #[inline]
  pub fn fields(&self) -> Option<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<PackedElementField<'a>>>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<PackedElementField>>>>(VoxelGrid::VT_FIELDS, None)}
  }
  /// Grid cell data, interpreted using `fields`, in depth-major, row-major (Z-Y-X) order.
  /// For the data element starting at byte offset i, the coordinates of its corner closest to the origin will be:
  /// 
  /// - z = i / slice_stride * cell_size.z
  /// - y = (i % slice_stride) / row_stride * cell_size.y
  /// - x = (i % row_stride) / cell_stride * cell_size.x
  #[inline]
  pub fn data(&self) -> Option<::flatbuffers::Vector<'a, u8>> {
    // Safety:
    // Created from valid Table for this object
    // which contains a valid value in this slot
    unsafe { self._tab.get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, u8>>>(VoxelGrid::VT_DATA, None)}
  }
}

impl ::flatbuffers::Verifiable for VoxelGrid<'_> {
  #[inline]
  fn run_verifier(
    v: &mut ::flatbuffers::Verifier, pos: usize
  ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
    v.visit_table(pos)?
     .visit_field::<Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("frame_id", Self::VT_FRAME_ID, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Pose>>("pose", Self::VT_POSE, false)?
     .visit_field::<u32>("row_count", Self::VT_ROW_COUNT, false)?
     .visit_field::<u32>("column_count", Self::VT_COLUMN_COUNT, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<Vector3>>("cell_size", Self::VT_CELL_SIZE, false)?
     .visit_field::<u32>("slice_stride", Self::VT_SLICE_STRIDE, false)?
     .visit_field::<u32>("row_stride", Self::VT_ROW_STRIDE, false)?
     .visit_field::<u32>("cell_stride", Self::VT_CELL_STRIDE, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, ::flatbuffers::ForwardsUOffset<PackedElementField>>>>("fields", Self::VT_FIELDS, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, u8>>>("data", Self::VT_DATA, false)?
     .finish();
    Ok(())
  }
}
pub struct VoxelGridArgs<'a> {
    pub timestamp: Option<&'a Time>,
    pub frame_id: Option<::flatbuffers::WIPOffset<&'a str>>,
    pub pose: Option<::flatbuffers::WIPOffset<Pose<'a>>>,
    pub row_count: u32,
    pub column_count: u32,
    pub cell_size: Option<::flatbuffers::WIPOffset<Vector3<'a>>>,
    pub slice_stride: u32,
    pub row_stride: u32,
    pub cell_stride: u32,
    pub fields: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, ::flatbuffers::ForwardsUOffset<PackedElementField<'a>>>>>,
    pub data: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, u8>>>,
}
impl<'a> Default for VoxelGridArgs<'a> {
  #[inline]
  fn default() -> Self {
    VoxelGridArgs {
      timestamp: None,
      frame_id: None,
      pose: None,
      row_count: 0,
      column_count: 0,
      cell_size: None,
      slice_stride: 0,
      row_stride: 0,
      cell_stride: 0,
      fields: None,
      data: None,
    }
  }
}

pub struct VoxelGridBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
  fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
  start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
}
impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> VoxelGridBuilder<'a, 'b, A> {
  #[inline]
  pub fn add_timestamp(&mut self, timestamp: &Time) {
    self.fbb_.push_slot_always::<&Time>(VoxelGrid::VT_TIMESTAMP, timestamp);
  }
  #[inline]
  pub fn add_frame_id(&mut self, frame_id: ::flatbuffers::WIPOffset<&'b  str>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(VoxelGrid::VT_FRAME_ID, frame_id);
  }
  #[inline]
  pub fn add_pose(&mut self, pose: ::flatbuffers::WIPOffset<Pose<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Pose>>(VoxelGrid::VT_POSE, pose);
  }
  #[inline]
  pub fn add_row_count(&mut self, row_count: u32) {
    self.fbb_.push_slot::<u32>(VoxelGrid::VT_ROW_COUNT, row_count, 0);
  }
  #[inline]
  pub fn add_column_count(&mut self, column_count: u32) {
    self.fbb_.push_slot::<u32>(VoxelGrid::VT_COLUMN_COUNT, column_count, 0);
  }
  #[inline]
  pub fn add_cell_size(&mut self, cell_size: ::flatbuffers::WIPOffset<Vector3<'b >>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<Vector3>>(VoxelGrid::VT_CELL_SIZE, cell_size);
  }
  #[inline]
  pub fn add_slice_stride(&mut self, slice_stride: u32) {
    self.fbb_.push_slot::<u32>(VoxelGrid::VT_SLICE_STRIDE, slice_stride, 0);
  }
  #[inline]
  pub fn add_row_stride(&mut self, row_stride: u32) {
    self.fbb_.push_slot::<u32>(VoxelGrid::VT_ROW_STRIDE, row_stride, 0);
  }
  #[inline]
  pub fn add_cell_stride(&mut self, cell_stride: u32) {
    self.fbb_.push_slot::<u32>(VoxelGrid::VT_CELL_STRIDE, cell_stride, 0);
  }
  #[inline]
  pub fn add_fields(&mut self, fields: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , ::flatbuffers::ForwardsUOffset<PackedElementField<'b >>>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(VoxelGrid::VT_FIELDS, fields);
  }
  #[inline]
  pub fn add_data(&mut self, data: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b , u8>>) {
    self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(VoxelGrid::VT_DATA, data);
  }
  #[inline]
  pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> VoxelGridBuilder<'a, 'b, A> {
    let start = _fbb.start_table();
    VoxelGridBuilder {
      fbb_: _fbb,
      start_: start,
    }
  }
  #[inline]
  pub fn finish(self) -> ::flatbuffers::WIPOffset<VoxelGrid<'a>> {
    let o = self.fbb_.end_table(self.start_);
    ::flatbuffers::WIPOffset::new(o.value())
  }
}

impl ::core::fmt::Debug for VoxelGrid<'_> {
  fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
    let mut ds = f.debug_struct("VoxelGrid");
      ds.field("timestamp", &self.timestamp());
      ds.field("frame_id", &self.frame_id());
      ds.field("pose", &self.pose());
      ds.field("row_count", &self.row_count());
      ds.field("column_count", &self.column_count());
      ds.field("cell_size", &self.cell_size());
      ds.field("slice_stride", &self.slice_stride());
      ds.field("row_stride", &self.row_stride());
      ds.field("cell_stride", &self.cell_stride());
      ds.field("fields", &self.fields());
      ds.field("data", &self.data());
      ds.finish()
  }
}
#[inline]
/// Verifies that a buffer of bytes contains a `VoxelGrid`
/// and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_voxel_grid_unchecked`.
pub fn root_as_voxel_grid(buf: &[u8]) -> Result<VoxelGrid<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root::<VoxelGrid>(buf)
}
#[inline]
/// Verifies that a buffer of bytes contains a size prefixed
/// `VoxelGrid` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `size_prefixed_root_as_voxel_grid_unchecked`.
pub fn size_prefixed_root_as_voxel_grid(buf: &[u8]) -> Result<VoxelGrid<'_>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root::<VoxelGrid>(buf)
}
#[inline]
/// Verifies, with the given options, that a buffer of bytes
/// contains a `VoxelGrid` and returns it.
/// Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_voxel_grid_unchecked`.
pub fn root_as_voxel_grid_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<VoxelGrid<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::root_with_opts::<VoxelGrid<'b>>(opts, buf)
}
#[inline]
/// Verifies, with the given verifier options, that a buffer of
/// bytes contains a size prefixed `VoxelGrid` and returns
/// it. Note that verification is still experimental and may not
/// catch every error, or be maximally performant. For the
/// previous, unchecked, behavior use
/// `root_as_voxel_grid_unchecked`.
pub fn size_prefixed_root_as_voxel_grid_with_opts<'b, 'o>(
  opts: &'o ::flatbuffers::VerifierOptions,
  buf: &'b [u8],
) -> Result<VoxelGrid<'b>, ::flatbuffers::InvalidFlatbuffer> {
  ::flatbuffers::size_prefixed_root_with_opts::<VoxelGrid<'b>>(opts, buf)
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a VoxelGrid and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid `VoxelGrid`.
pub unsafe fn root_as_voxel_grid_unchecked(buf: &[u8]) -> VoxelGrid<'_> {
  unsafe { ::flatbuffers::root_unchecked::<VoxelGrid>(buf) }
}
#[inline]
/// Assumes, without verification, that a buffer of bytes contains a size prefixed VoxelGrid and returns it.
/// # Safety
/// Callers must trust the given bytes do indeed contain a valid size prefixed `VoxelGrid`.
pub unsafe fn size_prefixed_root_as_voxel_grid_unchecked(buf: &[u8]) -> VoxelGrid<'_> {
  unsafe { ::flatbuffers::size_prefixed_root_unchecked::<VoxelGrid>(buf) }
}
#[inline]
pub fn finish_voxel_grid_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
    fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
    root: ::flatbuffers::WIPOffset<VoxelGrid<'a>>) {
  fbb.finish(root, None);
}

#[inline]
pub fn finish_size_prefixed_voxel_grid_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>, root: ::flatbuffers::WIPOffset<VoxelGrid<'a>>) {
  fbb.finish_size_prefixed(root, None);
}


} // pub mod foxglove
