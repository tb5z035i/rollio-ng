// AUTO-MERGED by tools/merge_fbs.py — DO NOT EDIT BY HAND.
// Inputs: src/fbs/*_generated.rs (verbatim flatc output)
// Namespace: discover

#![allow(unused_imports, dead_code, non_snake_case, clippy::all)]

extern crate alloc;

pub use crate::fbs_foxglove::foxglove;

pub mod discover {

    // ===== from Imu_generated.rs =====

    pub enum ImuOffset {}
    #[derive(Copy, Clone, PartialEq)]

    /// IMU (Inertial Measurement Unit) sensor data containing orientation,
    /// angular velocity, and linear acceleration.
    pub struct Imu<'a> {
        pub _tab: ::flatbuffers::Table<'a>,
    }

    impl<'a> ::flatbuffers::Follow<'a> for Imu<'a> {
        type Inner = Imu<'a>;
        #[inline]
        unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
            Self {
                _tab: unsafe { ::flatbuffers::Table::new(buf, loc) },
            }
        }
    }

    impl<'a> Imu<'a> {
        pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
        pub const VT_FRAME_ID: ::flatbuffers::VOffsetT = 6;
        pub const VT_ORIENTATION: ::flatbuffers::VOffsetT = 8;
        pub const VT_ANGULAR_VELOCITY: ::flatbuffers::VOffsetT = 10;
        pub const VT_LINEAR_ACCELERATION: ::flatbuffers::VOffsetT = 12;

        #[inline]
        pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
            Imu { _tab: table }
        }
        #[allow(unused_mut)]
        pub fn create<
            'bldr: 'args,
            'args: 'mut_bldr,
            'mut_bldr,
            A: ::flatbuffers::Allocator + 'bldr,
        >(
            _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
            args: &'args ImuArgs<'args>,
        ) -> ::flatbuffers::WIPOffset<Imu<'bldr>> {
            let mut builder = ImuBuilder::new(_fbb);
            if let Some(x) = args.linear_acceleration {
                builder.add_linear_acceleration(x);
            }
            if let Some(x) = args.angular_velocity {
                builder.add_angular_velocity(x);
            }
            if let Some(x) = args.orientation {
                builder.add_orientation(x);
            }
            if let Some(x) = args.frame_id {
                builder.add_frame_id(x);
            }
            if let Some(x) = args.timestamp {
                builder.add_timestamp(x);
            }
            builder.finish()
        }

        /// Timestamp of the IMU measurement
        #[inline]
        pub fn timestamp(&self) -> Option<&'a super::foxglove::Time> {
            // Safety:
            // Created from valid Table for this object
            // which contains a valid value in this slot
            unsafe {
                self._tab
                    .get::<super::foxglove::Time>(Imu::VT_TIMESTAMP, None)
            }
        }
        /// Frame of reference
        #[inline]
        pub fn frame_id(&self) -> Option<&'a str> {
            // Safety:
            // Created from valid Table for this object
            // which contains a valid value in this slot
            unsafe {
                self._tab
                    .get::<::flatbuffers::ForwardsUOffset<&str>>(Imu::VT_FRAME_ID, None)
            }
        }
        /// Orientation as a quaternion (x, y, z, w)
        #[inline]
        pub fn orientation(&self) -> Option<super::foxglove::Quaternion<'a>> {
            // Safety:
            // Created from valid Table for this object
            // which contains a valid value in this slot
            unsafe {
                self._tab
                    .get::<::flatbuffers::ForwardsUOffset<super::foxglove::Quaternion>>(
                        Imu::VT_ORIENTATION,
                        None,
                    )
            }
        }
        /// Angular velocity in rad/s
        #[inline]
        pub fn angular_velocity(&self) -> Option<super::foxglove::Vector3<'a>> {
            // Safety:
            // Created from valid Table for this object
            // which contains a valid value in this slot
            unsafe {
                self._tab
                    .get::<::flatbuffers::ForwardsUOffset<super::foxglove::Vector3>>(
                        Imu::VT_ANGULAR_VELOCITY,
                        None,
                    )
            }
        }
        /// Linear acceleration in m/s²
        #[inline]
        pub fn linear_acceleration(&self) -> Option<super::foxglove::Vector3<'a>> {
            // Safety:
            // Created from valid Table for this object
            // which contains a valid value in this slot
            unsafe {
                self._tab
                    .get::<::flatbuffers::ForwardsUOffset<super::foxglove::Vector3>>(
                        Imu::VT_LINEAR_ACCELERATION,
                        None,
                    )
            }
        }
    }

    impl ::flatbuffers::Verifiable for Imu<'_> {
        #[inline]
        fn run_verifier(
            v: &mut ::flatbuffers::Verifier,
            pos: usize,
        ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
            v.visit_table(pos)?
                .visit_field::<super::foxglove::Time>("timestamp", Self::VT_TIMESTAMP, false)?
                .visit_field::<::flatbuffers::ForwardsUOffset<&str>>(
                    "frame_id",
                    Self::VT_FRAME_ID,
                    false,
                )?
                .visit_field::<::flatbuffers::ForwardsUOffset<super::foxglove::Quaternion>>(
                    "orientation",
                    Self::VT_ORIENTATION,
                    false,
                )?
                .visit_field::<::flatbuffers::ForwardsUOffset<super::foxglove::Vector3>>(
                    "angular_velocity",
                    Self::VT_ANGULAR_VELOCITY,
                    false,
                )?
                .visit_field::<::flatbuffers::ForwardsUOffset<super::foxglove::Vector3>>(
                    "linear_acceleration",
                    Self::VT_LINEAR_ACCELERATION,
                    false,
                )?
                .finish();
            Ok(())
        }
    }
    pub struct ImuArgs<'a> {
        pub timestamp: Option<&'a super::foxglove::Time>,
        pub frame_id: Option<::flatbuffers::WIPOffset<&'a str>>,
        pub orientation: Option<::flatbuffers::WIPOffset<super::foxglove::Quaternion<'a>>>,
        pub angular_velocity: Option<::flatbuffers::WIPOffset<super::foxglove::Vector3<'a>>>,
        pub linear_acceleration: Option<::flatbuffers::WIPOffset<super::foxglove::Vector3<'a>>>,
    }
    impl<'a> Default for ImuArgs<'a> {
        #[inline]
        fn default() -> Self {
            ImuArgs {
                timestamp: None,
                frame_id: None,
                orientation: None,
                angular_velocity: None,
                linear_acceleration: None,
            }
        }
    }

    pub struct ImuBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
        fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
        start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
    }
    impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> ImuBuilder<'a, 'b, A> {
        #[inline]
        pub fn add_timestamp(&mut self, timestamp: &super::foxglove::Time) {
            self.fbb_
                .push_slot_always::<&super::foxglove::Time>(Imu::VT_TIMESTAMP, timestamp);
        }
        #[inline]
        pub fn add_frame_id(&mut self, frame_id: ::flatbuffers::WIPOffset<&'b str>) {
            self.fbb_
                .push_slot_always::<::flatbuffers::WIPOffset<_>>(Imu::VT_FRAME_ID, frame_id);
        }
        #[inline]
        pub fn add_orientation(
            &mut self,
            orientation: ::flatbuffers::WIPOffset<super::foxglove::Quaternion<'b>>,
        ) {
            self.fbb_
                .push_slot_always::<::flatbuffers::WIPOffset<super::foxglove::Quaternion>>(
                    Imu::VT_ORIENTATION,
                    orientation,
                );
        }
        #[inline]
        pub fn add_angular_velocity(
            &mut self,
            angular_velocity: ::flatbuffers::WIPOffset<super::foxglove::Vector3<'b>>,
        ) {
            self.fbb_
                .push_slot_always::<::flatbuffers::WIPOffset<super::foxglove::Vector3>>(
                    Imu::VT_ANGULAR_VELOCITY,
                    angular_velocity,
                );
        }
        #[inline]
        pub fn add_linear_acceleration(
            &mut self,
            linear_acceleration: ::flatbuffers::WIPOffset<super::foxglove::Vector3<'b>>,
        ) {
            self.fbb_
                .push_slot_always::<::flatbuffers::WIPOffset<super::foxglove::Vector3>>(
                    Imu::VT_LINEAR_ACCELERATION,
                    linear_acceleration,
                );
        }
        #[inline]
        pub fn new(_fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>) -> ImuBuilder<'a, 'b, A> {
            let start = _fbb.start_table();
            ImuBuilder {
                fbb_: _fbb,
                start_: start,
            }
        }
        #[inline]
        pub fn finish(self) -> ::flatbuffers::WIPOffset<Imu<'a>> {
            let o = self.fbb_.end_table(self.start_);
            ::flatbuffers::WIPOffset::new(o.value())
        }
    }

    impl ::core::fmt::Debug for Imu<'_> {
        fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
            let mut ds = f.debug_struct("Imu");
            ds.field("timestamp", &self.timestamp());
            ds.field("frame_id", &self.frame_id());
            ds.field("orientation", &self.orientation());
            ds.field("angular_velocity", &self.angular_velocity());
            ds.field("linear_acceleration", &self.linear_acceleration());
            ds.finish()
        }
    }
    #[inline]
    /// Verifies that a buffer of bytes contains a `Imu`
    /// and returns it.
    /// Note that verification is still experimental and may not
    /// catch every error, or be maximally performant. For the
    /// previous, unchecked, behavior use
    /// `root_as_imu_unchecked`.
    pub fn root_as_imu(buf: &[u8]) -> Result<Imu<'_>, ::flatbuffers::InvalidFlatbuffer> {
        ::flatbuffers::root::<Imu>(buf)
    }
    #[inline]
    /// Verifies that a buffer of bytes contains a size prefixed
    /// `Imu` and returns it.
    /// Note that verification is still experimental and may not
    /// catch every error, or be maximally performant. For the
    /// previous, unchecked, behavior use
    /// `size_prefixed_root_as_imu_unchecked`.
    pub fn size_prefixed_root_as_imu(
        buf: &[u8],
    ) -> Result<Imu<'_>, ::flatbuffers::InvalidFlatbuffer> {
        ::flatbuffers::size_prefixed_root::<Imu>(buf)
    }
    #[inline]
    /// Verifies, with the given options, that a buffer of bytes
    /// contains a `Imu` and returns it.
    /// Note that verification is still experimental and may not
    /// catch every error, or be maximally performant. For the
    /// previous, unchecked, behavior use
    /// `root_as_imu_unchecked`.
    pub fn root_as_imu_with_opts<'b, 'o>(
        opts: &'o ::flatbuffers::VerifierOptions,
        buf: &'b [u8],
    ) -> Result<Imu<'b>, ::flatbuffers::InvalidFlatbuffer> {
        ::flatbuffers::root_with_opts::<Imu<'b>>(opts, buf)
    }
    #[inline]
    /// Verifies, with the given verifier options, that a buffer of
    /// bytes contains a size prefixed `Imu` and returns
    /// it. Note that verification is still experimental and may not
    /// catch every error, or be maximally performant. For the
    /// previous, unchecked, behavior use
    /// `root_as_imu_unchecked`.
    pub fn size_prefixed_root_as_imu_with_opts<'b, 'o>(
        opts: &'o ::flatbuffers::VerifierOptions,
        buf: &'b [u8],
    ) -> Result<Imu<'b>, ::flatbuffers::InvalidFlatbuffer> {
        ::flatbuffers::size_prefixed_root_with_opts::<Imu<'b>>(opts, buf)
    }
    #[inline]
    /// Assumes, without verification, that a buffer of bytes contains a Imu and returns it.
    /// # Safety
    /// Callers must trust the given bytes do indeed contain a valid `Imu`.
    pub unsafe fn root_as_imu_unchecked(buf: &[u8]) -> Imu<'_> {
        unsafe { ::flatbuffers::root_unchecked::<Imu>(buf) }
    }
    #[inline]
    /// Assumes, without verification, that a buffer of bytes contains a size prefixed Imu and returns it.
    /// # Safety
    /// Callers must trust the given bytes do indeed contain a valid size prefixed `Imu`.
    pub unsafe fn size_prefixed_root_as_imu_unchecked(buf: &[u8]) -> Imu<'_> {
        unsafe { ::flatbuffers::size_prefixed_root_unchecked::<Imu>(buf) }
    }
    #[inline]
    pub fn finish_imu_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
        fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
        root: ::flatbuffers::WIPOffset<Imu<'a>>,
    ) {
        fbb.finish(root, None);
    }

    #[inline]
    pub fn finish_size_prefixed_imu_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
        fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
        root: ::flatbuffers::WIPOffset<Imu<'a>>,
    ) {
        fbb.finish_size_prefixed(root, None);
    }

    // ===== from TactileData_generated.rs =====

    /// A single tactile point with 6 dimensions: position (x, y, z) and force (fx, fy, fz)
    // struct TactilePoint, aligned to 4
    #[repr(transparent)]
    #[derive(Clone, Copy, PartialEq)]
    pub struct TactilePoint(pub [u8; 24]);
    impl Default for TactilePoint {
        fn default() -> Self {
            Self([0; 24])
        }
    }
    impl ::core::fmt::Debug for TactilePoint {
        fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
            f.debug_struct("TactilePoint")
                .field("x", &self.x())
                .field("y", &self.y())
                .field("z", &self.z())
                .field("fx", &self.fx())
                .field("fy", &self.fy())
                .field("fz", &self.fz())
                .finish()
        }
    }

    impl ::flatbuffers::SimpleToVerifyInSlice for TactilePoint {}
    impl<'a> ::flatbuffers::Follow<'a> for TactilePoint {
        type Inner = &'a TactilePoint;
        #[inline]
        unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
            unsafe { <&'a TactilePoint>::follow(buf, loc) }
        }
    }
    impl<'a> ::flatbuffers::Follow<'a> for &'a TactilePoint {
        type Inner = &'a TactilePoint;
        #[inline]
        unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
            unsafe { ::flatbuffers::follow_cast_ref::<TactilePoint>(buf, loc) }
        }
    }
    impl<'b> ::flatbuffers::Push for TactilePoint {
        type Output = TactilePoint;
        #[inline]
        unsafe fn push(&self, dst: &mut [u8], _written_len: usize) {
            let src = unsafe {
                ::core::slice::from_raw_parts(
                    self as *const TactilePoint as *const u8,
                    <Self as ::flatbuffers::Push>::size(),
                )
            };
            dst.copy_from_slice(src);
        }
        #[inline]
        fn alignment() -> ::flatbuffers::PushAlignment {
            ::flatbuffers::PushAlignment::new(4)
        }
    }

    impl<'a> ::flatbuffers::Verifiable for TactilePoint {
        #[inline]
        fn run_verifier(
            v: &mut ::flatbuffers::Verifier,
            pos: usize,
        ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
            v.in_buffer::<Self>(pos)
        }
    }

    impl<'a> TactilePoint {
        #[allow(clippy::too_many_arguments)]
        pub fn new(x: f32, y: f32, z: f32, fx: f32, fy: f32, fz: f32) -> Self {
            let mut s = Self([0; 24]);
            s.set_x(x);
            s.set_y(y);
            s.set_z(z);
            s.set_fx(fx);
            s.set_fy(fy);
            s.set_fz(fz);
            s
        }

        /// Position x coordinate
        pub fn x(&self) -> f32 {
            let mut mem =
                ::core::mem::MaybeUninit::<<f32 as ::flatbuffers::EndianScalar>::Scalar>::uninit();
            // Safety:
            // Created from a valid Table for this object
            // Which contains a valid value in this slot
            ::flatbuffers::EndianScalar::from_little_endian(unsafe {
                ::core::ptr::copy_nonoverlapping(
                    self.0[0..].as_ptr(),
                    mem.as_mut_ptr() as *mut u8,
                    ::core::mem::size_of::<<f32 as ::flatbuffers::EndianScalar>::Scalar>(),
                );
                mem.assume_init()
            })
        }

        pub fn set_x(&mut self, x: f32) {
            let x_le = ::flatbuffers::EndianScalar::to_little_endian(x);
            // Safety:
            // Created from a valid Table for this object
            // Which contains a valid value in this slot
            unsafe {
                ::core::ptr::copy_nonoverlapping(
                    &x_le as *const _ as *const u8,
                    self.0[0..].as_mut_ptr(),
                    ::core::mem::size_of::<<f32 as ::flatbuffers::EndianScalar>::Scalar>(),
                );
            }
        }

        /// Position y coordinate
        pub fn y(&self) -> f32 {
            let mut mem =
                ::core::mem::MaybeUninit::<<f32 as ::flatbuffers::EndianScalar>::Scalar>::uninit();
            // Safety:
            // Created from a valid Table for this object
            // Which contains a valid value in this slot
            ::flatbuffers::EndianScalar::from_little_endian(unsafe {
                ::core::ptr::copy_nonoverlapping(
                    self.0[4..].as_ptr(),
                    mem.as_mut_ptr() as *mut u8,
                    ::core::mem::size_of::<<f32 as ::flatbuffers::EndianScalar>::Scalar>(),
                );
                mem.assume_init()
            })
        }

        pub fn set_y(&mut self, x: f32) {
            let x_le = ::flatbuffers::EndianScalar::to_little_endian(x);
            // Safety:
            // Created from a valid Table for this object
            // Which contains a valid value in this slot
            unsafe {
                ::core::ptr::copy_nonoverlapping(
                    &x_le as *const _ as *const u8,
                    self.0[4..].as_mut_ptr(),
                    ::core::mem::size_of::<<f32 as ::flatbuffers::EndianScalar>::Scalar>(),
                );
            }
        }

        /// Position z coordinate
        pub fn z(&self) -> f32 {
            let mut mem =
                ::core::mem::MaybeUninit::<<f32 as ::flatbuffers::EndianScalar>::Scalar>::uninit();
            // Safety:
            // Created from a valid Table for this object
            // Which contains a valid value in this slot
            ::flatbuffers::EndianScalar::from_little_endian(unsafe {
                ::core::ptr::copy_nonoverlapping(
                    self.0[8..].as_ptr(),
                    mem.as_mut_ptr() as *mut u8,
                    ::core::mem::size_of::<<f32 as ::flatbuffers::EndianScalar>::Scalar>(),
                );
                mem.assume_init()
            })
        }

        pub fn set_z(&mut self, x: f32) {
            let x_le = ::flatbuffers::EndianScalar::to_little_endian(x);
            // Safety:
            // Created from a valid Table for this object
            // Which contains a valid value in this slot
            unsafe {
                ::core::ptr::copy_nonoverlapping(
                    &x_le as *const _ as *const u8,
                    self.0[8..].as_mut_ptr(),
                    ::core::mem::size_of::<<f32 as ::flatbuffers::EndianScalar>::Scalar>(),
                );
            }
        }

        /// Force x component
        pub fn fx(&self) -> f32 {
            let mut mem =
                ::core::mem::MaybeUninit::<<f32 as ::flatbuffers::EndianScalar>::Scalar>::uninit();
            // Safety:
            // Created from a valid Table for this object
            // Which contains a valid value in this slot
            ::flatbuffers::EndianScalar::from_little_endian(unsafe {
                ::core::ptr::copy_nonoverlapping(
                    self.0[12..].as_ptr(),
                    mem.as_mut_ptr() as *mut u8,
                    ::core::mem::size_of::<<f32 as ::flatbuffers::EndianScalar>::Scalar>(),
                );
                mem.assume_init()
            })
        }

        pub fn set_fx(&mut self, x: f32) {
            let x_le = ::flatbuffers::EndianScalar::to_little_endian(x);
            // Safety:
            // Created from a valid Table for this object
            // Which contains a valid value in this slot
            unsafe {
                ::core::ptr::copy_nonoverlapping(
                    &x_le as *const _ as *const u8,
                    self.0[12..].as_mut_ptr(),
                    ::core::mem::size_of::<<f32 as ::flatbuffers::EndianScalar>::Scalar>(),
                );
            }
        }

        /// Force y component
        pub fn fy(&self) -> f32 {
            let mut mem =
                ::core::mem::MaybeUninit::<<f32 as ::flatbuffers::EndianScalar>::Scalar>::uninit();
            // Safety:
            // Created from a valid Table for this object
            // Which contains a valid value in this slot
            ::flatbuffers::EndianScalar::from_little_endian(unsafe {
                ::core::ptr::copy_nonoverlapping(
                    self.0[16..].as_ptr(),
                    mem.as_mut_ptr() as *mut u8,
                    ::core::mem::size_of::<<f32 as ::flatbuffers::EndianScalar>::Scalar>(),
                );
                mem.assume_init()
            })
        }

        pub fn set_fy(&mut self, x: f32) {
            let x_le = ::flatbuffers::EndianScalar::to_little_endian(x);
            // Safety:
            // Created from a valid Table for this object
            // Which contains a valid value in this slot
            unsafe {
                ::core::ptr::copy_nonoverlapping(
                    &x_le as *const _ as *const u8,
                    self.0[16..].as_mut_ptr(),
                    ::core::mem::size_of::<<f32 as ::flatbuffers::EndianScalar>::Scalar>(),
                );
            }
        }

        /// Force z component
        pub fn fz(&self) -> f32 {
            let mut mem =
                ::core::mem::MaybeUninit::<<f32 as ::flatbuffers::EndianScalar>::Scalar>::uninit();
            // Safety:
            // Created from a valid Table for this object
            // Which contains a valid value in this slot
            ::flatbuffers::EndianScalar::from_little_endian(unsafe {
                ::core::ptr::copy_nonoverlapping(
                    self.0[20..].as_ptr(),
                    mem.as_mut_ptr() as *mut u8,
                    ::core::mem::size_of::<<f32 as ::flatbuffers::EndianScalar>::Scalar>(),
                );
                mem.assume_init()
            })
        }

        pub fn set_fz(&mut self, x: f32) {
            let x_le = ::flatbuffers::EndianScalar::to_little_endian(x);
            // Safety:
            // Created from a valid Table for this object
            // Which contains a valid value in this slot
            unsafe {
                ::core::ptr::copy_nonoverlapping(
                    &x_le as *const _ as *const u8,
                    self.0[20..].as_mut_ptr(),
                    ::core::mem::size_of::<<f32 as ::flatbuffers::EndianScalar>::Scalar>(),
                );
            }
        }
    }

    pub enum TactileDataOffset {}
    #[derive(Copy, Clone, PartialEq)]

    /// Tactile sensor data from PaXini multi-dimensional tactile sensors.
    /// Each point contains position and force vectors (6 dimensions total).
    pub struct TactileData<'a> {
        pub _tab: ::flatbuffers::Table<'a>,
    }

    impl<'a> ::flatbuffers::Follow<'a> for TactileData<'a> {
        type Inner = TactileData<'a>;
        #[inline]
        unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
            Self {
                _tab: unsafe { ::flatbuffers::Table::new(buf, loc) },
            }
        }
    }

    impl<'a> TactileData<'a> {
        pub const VT_TIMESTAMP: ::flatbuffers::VOffsetT = 4;
        pub const VT_FRAME_ID: ::flatbuffers::VOffsetT = 6;
        pub const VT_POINTS: ::flatbuffers::VOffsetT = 8;

        #[inline]
        pub unsafe fn init_from_table(table: ::flatbuffers::Table<'a>) -> Self {
            TactileData { _tab: table }
        }
        #[allow(unused_mut)]
        pub fn create<
            'bldr: 'args,
            'args: 'mut_bldr,
            'mut_bldr,
            A: ::flatbuffers::Allocator + 'bldr,
        >(
            _fbb: &'mut_bldr mut ::flatbuffers::FlatBufferBuilder<'bldr, A>,
            args: &'args TactileDataArgs<'args>,
        ) -> ::flatbuffers::WIPOffset<TactileData<'bldr>> {
            let mut builder = TactileDataBuilder::new(_fbb);
            if let Some(x) = args.points {
                builder.add_points(x);
            }
            if let Some(x) = args.frame_id {
                builder.add_frame_id(x);
            }
            if let Some(x) = args.timestamp {
                builder.add_timestamp(x);
            }
            builder.finish()
        }

        /// Timestamp of the tactile measurement
        #[inline]
        pub fn timestamp(&self) -> Option<&'a super::foxglove::Time> {
            // Safety:
            // Created from valid Table for this object
            // which contains a valid value in this slot
            unsafe {
                self._tab
                    .get::<super::foxglove::Time>(TactileData::VT_TIMESTAMP, None)
            }
        }
        /// Frame of reference
        #[inline]
        pub fn frame_id(&self) -> Option<&'a str> {
            // Safety:
            // Created from valid Table for this object
            // which contains a valid value in this slot
            unsafe {
                self._tab
                    .get::<::flatbuffers::ForwardsUOffset<&str>>(TactileData::VT_FRAME_ID, None)
            }
        }
        /// Array of tactile contact points
        #[inline]
        pub fn points(&self) -> Option<::flatbuffers::Vector<'a, TactilePoint>> {
            // Safety:
            // Created from valid Table for this object
            // which contains a valid value in this slot
            unsafe {
                self._tab
                    .get::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'a, TactilePoint>>>(
                        TactileData::VT_POINTS,
                        None,
                    )
            }
        }
    }

    impl ::flatbuffers::Verifiable for TactileData<'_> {
        #[inline]
        fn run_verifier(
            v: &mut ::flatbuffers::Verifier,
            pos: usize,
        ) -> Result<(), ::flatbuffers::InvalidFlatbuffer> {
            v.visit_table(pos)?
     .visit_field::<super::foxglove::Time>("timestamp", Self::VT_TIMESTAMP, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<&str>>("frame_id", Self::VT_FRAME_ID, false)?
     .visit_field::<::flatbuffers::ForwardsUOffset<::flatbuffers::Vector<'_, TactilePoint>>>("points", Self::VT_POINTS, false)?
     .finish();
            Ok(())
        }
    }
    pub struct TactileDataArgs<'a> {
        pub timestamp: Option<&'a super::foxglove::Time>,
        pub frame_id: Option<::flatbuffers::WIPOffset<&'a str>>,
        pub points: Option<::flatbuffers::WIPOffset<::flatbuffers::Vector<'a, TactilePoint>>>,
    }
    impl<'a> Default for TactileDataArgs<'a> {
        #[inline]
        fn default() -> Self {
            TactileDataArgs {
                timestamp: None,
                frame_id: None,
                points: None,
            }
        }
    }

    pub struct TactileDataBuilder<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> {
        fbb_: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
        start_: ::flatbuffers::WIPOffset<::flatbuffers::TableUnfinishedWIPOffset>,
    }
    impl<'a: 'b, 'b, A: ::flatbuffers::Allocator + 'a> TactileDataBuilder<'a, 'b, A> {
        #[inline]
        pub fn add_timestamp(&mut self, timestamp: &super::foxglove::Time) {
            self.fbb_
                .push_slot_always::<&super::foxglove::Time>(TactileData::VT_TIMESTAMP, timestamp);
        }
        #[inline]
        pub fn add_frame_id(&mut self, frame_id: ::flatbuffers::WIPOffset<&'b str>) {
            self.fbb_.push_slot_always::<::flatbuffers::WIPOffset<_>>(
                TactileData::VT_FRAME_ID,
                frame_id,
            );
        }
        #[inline]
        pub fn add_points(
            &mut self,
            points: ::flatbuffers::WIPOffset<::flatbuffers::Vector<'b, TactilePoint>>,
        ) {
            self.fbb_
                .push_slot_always::<::flatbuffers::WIPOffset<_>>(TactileData::VT_POINTS, points);
        }
        #[inline]
        pub fn new(
            _fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
        ) -> TactileDataBuilder<'a, 'b, A> {
            let start = _fbb.start_table();
            TactileDataBuilder {
                fbb_: _fbb,
                start_: start,
            }
        }
        #[inline]
        pub fn finish(self) -> ::flatbuffers::WIPOffset<TactileData<'a>> {
            let o = self.fbb_.end_table(self.start_);
            ::flatbuffers::WIPOffset::new(o.value())
        }
    }

    impl ::core::fmt::Debug for TactileData<'_> {
        fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
            let mut ds = f.debug_struct("TactileData");
            ds.field("timestamp", &self.timestamp());
            ds.field("frame_id", &self.frame_id());
            ds.field("points", &self.points());
            ds.finish()
        }
    }
    #[inline]
    /// Verifies that a buffer of bytes contains a `TactileData`
    /// and returns it.
    /// Note that verification is still experimental and may not
    /// catch every error, or be maximally performant. For the
    /// previous, unchecked, behavior use
    /// `root_as_tactile_data_unchecked`.
    pub fn root_as_tactile_data(
        buf: &[u8],
    ) -> Result<TactileData<'_>, ::flatbuffers::InvalidFlatbuffer> {
        ::flatbuffers::root::<TactileData>(buf)
    }
    #[inline]
    /// Verifies that a buffer of bytes contains a size prefixed
    /// `TactileData` and returns it.
    /// Note that verification is still experimental and may not
    /// catch every error, or be maximally performant. For the
    /// previous, unchecked, behavior use
    /// `size_prefixed_root_as_tactile_data_unchecked`.
    pub fn size_prefixed_root_as_tactile_data(
        buf: &[u8],
    ) -> Result<TactileData<'_>, ::flatbuffers::InvalidFlatbuffer> {
        ::flatbuffers::size_prefixed_root::<TactileData>(buf)
    }
    #[inline]
    /// Verifies, with the given options, that a buffer of bytes
    /// contains a `TactileData` and returns it.
    /// Note that verification is still experimental and may not
    /// catch every error, or be maximally performant. For the
    /// previous, unchecked, behavior use
    /// `root_as_tactile_data_unchecked`.
    pub fn root_as_tactile_data_with_opts<'b, 'o>(
        opts: &'o ::flatbuffers::VerifierOptions,
        buf: &'b [u8],
    ) -> Result<TactileData<'b>, ::flatbuffers::InvalidFlatbuffer> {
        ::flatbuffers::root_with_opts::<TactileData<'b>>(opts, buf)
    }
    #[inline]
    /// Verifies, with the given verifier options, that a buffer of
    /// bytes contains a size prefixed `TactileData` and returns
    /// it. Note that verification is still experimental and may not
    /// catch every error, or be maximally performant. For the
    /// previous, unchecked, behavior use
    /// `root_as_tactile_data_unchecked`.
    pub fn size_prefixed_root_as_tactile_data_with_opts<'b, 'o>(
        opts: &'o ::flatbuffers::VerifierOptions,
        buf: &'b [u8],
    ) -> Result<TactileData<'b>, ::flatbuffers::InvalidFlatbuffer> {
        ::flatbuffers::size_prefixed_root_with_opts::<TactileData<'b>>(opts, buf)
    }
    #[inline]
    /// Assumes, without verification, that a buffer of bytes contains a TactileData and returns it.
    /// # Safety
    /// Callers must trust the given bytes do indeed contain a valid `TactileData`.
    pub unsafe fn root_as_tactile_data_unchecked(buf: &[u8]) -> TactileData<'_> {
        unsafe { ::flatbuffers::root_unchecked::<TactileData>(buf) }
    }
    #[inline]
    /// Assumes, without verification, that a buffer of bytes contains a size prefixed TactileData and returns it.
    /// # Safety
    /// Callers must trust the given bytes do indeed contain a valid size prefixed `TactileData`.
    pub unsafe fn size_prefixed_root_as_tactile_data_unchecked(buf: &[u8]) -> TactileData<'_> {
        unsafe { ::flatbuffers::size_prefixed_root_unchecked::<TactileData>(buf) }
    }
    #[inline]
    pub fn finish_tactile_data_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
        fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
        root: ::flatbuffers::WIPOffset<TactileData<'a>>,
    ) {
        fbb.finish(root, None);
    }

    #[inline]
    pub fn finish_size_prefixed_tactile_data_buffer<'a, 'b, A: ::flatbuffers::Allocator + 'a>(
        fbb: &'b mut ::flatbuffers::FlatBufferBuilder<'a, A>,
        root: ::flatbuffers::WIPOffset<TactileData<'a>>,
    ) {
        fbb.finish_size_prefixed(root, None);
    }
} // pub mod discover
