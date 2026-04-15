It seems that the device implementation is too coupling right now. Ideally, the framework should handle the data on the iceoryx bus, instead of caring and hardcoding so much in the framework it self. I would like to propose another way so that adding support for new device would be much easier:

The device does not have a specific "type" anymore. An identified device can have multiple channels, where channels can have camera/robot as types. imu may be added as a type for channels in the future. In this case, AIRBOT Play + AIRBOT G2/E2 would be regarded as a single device, with channels: arm (robot channel) and g2 (robot channel) or e2 (robot channel). A realsense camera is a single device with multiple channels: color (camera channel), depth (camera channel), infrared (camera channel). For each channel of device, it should have a unique id across different devices. For examples, say, arm has an id of 0, G2 has an id of 1, and E2 has an id of 2, then the AIRBOT Play + G2 would be regarded as a single device, with channels: arm (robot channel with id 0) and g2 (robot channel with id 1). The channel id and channel name should be unique across different devices, but whether or not a device currently has a specific channel is decided by the vendor. Treating the cameras and robots uniformly is due to a mixed form of devices that provide cameras and robots in the same device.

Each channel has its own modes. For cameras, the modes are Enabled/Disabled. For robots, the modes are FreeDrive/CommandFollowing/Disabled.

The support of a device is given by an executable that can be found in PATH. The framework would know what devices are supported provided an executable name list.

The executable should be able to run these subcommands:
1. probe (partly like current implementation), which provide a human-friendly output for current devices ID, and the supported channels of the device. And with --json it gives a json list of ids for the devices. 
  The detailed form of the device id is decided by the vendor.
  For current supported devices and their ID:
  - AIRBOT Play (with possible mounted G2 or E2): SN
  - Realsense: SN
  - V4L2: bus info like usb-0000:af:00.0-4. This is a somewhat special case since not all V4L2 cameras has a vendor-assigned ID.
  - Mock Camera: a given ID.

2. validate (like current requirement), given an device ID and a channel (id) list , cross check the existence of the device. This should be normaly output: result and reason, with 0 / 1 as return values for programatic use. With --json it gives a json of the aforementioned.
  The detailed cross check method should be decided by the vendor. Not only the existence, but also the currently available channels should be checked.

3. query (replacing current capabilities). The cases are different for cameras and robots:
  - device type (AIRBOT Play, Realsense, V4L2, Mock Camera)
  - device id
  - supported channels (a list of channel ids and corresponding channel types (robot/camera) and channel names (like for AIRBOT Play, arm/g2/e2; for realsense, color/depth/infrared), and channel information). Channel information:
    - (for each camera channels) support profile (a list of tuples, each tuple contains: width, height, fps, pixel_format. Pixel formats should at least contain YUYV/MJPG (v4l2), and other possible formats in realsense)
    - (for each robot channels) support control modes (at least one from: free-drive/command-following)
    - (for each robot channels) dof
    - (for each robot channels) supported states (selecting at least one from: joint position, joint velocity, joint effort, end-effector pose, end-effector twist, end-effector wrench, parallel position, parallel velocity, parallel effort)
    - (for each robot channels) supported control interfaces (selecting at least one from: joint MIT, joint position, end-effector pose, parallel MIT, parallel position)
    - (for each robot channels) default control frequency (in Hz)
    - a dict of other optional channel information provided by the vendor
  - a dict of other optional device information provided by the vendor
  query subcommand should produce human-friendly output, and with --json it gives a json list of devices.

4. run (partly like current run, but with uniform behavior) run should accept a local TOML config file with --config or direct TOML string with --config-inline.
   The service should listen / expose these services / events on the iceoryx bus: (device_name is given in the config, and channel_name is the predefined name of the channel)
   1) {device_name}/info [type: DeviceInfo]: service answering to requests, and provide a shared-structured response with contents similar to query subcommand
   2) {device_name}/shutdown: event listener, after receiving the event, the service should shutdown gracefully.
   3) {device_name}/{channel_name}/status [type: Status]: iceoryx channel publishing enum from Okay/Degraded/Error
   4) {device_name}/{channel_name}/info/mode [type: Mode]: iceoryx channel publishing the current mode. For cameras, the state is Enabled/Disabled. For robots, the state is FreeDrive/CommandFollowing/Disabled.
   5) {device_name}/{channel_name}/control/mode [type: Mode]: iceoryx2 service answering to requests to switch the mode. Available modes: Enabled/Disabled (for cameras), FreeDrive/CommandFollowing/Disabled (for robots)
   6) (for camera channels) {device_name}/{channel_name}/info/profile [type: Profile]: iceoryx2 channel publishing the current profile.
   6) (for camera channels) {device_name}/{channel_name}/control/profile [type: Profile]: iceoryx2 service answering to requests to set the profile to use. The profile is a tuple of width, height, fps, pixel_format. The profile should be effective after the camera is disabled and then enabled.
   7) (for camera channels) {device_name}/{channel_name}/frames[type: CameraFrameHeader + [u8]]: iceoryx2 channel publishing frames to the iceoryx bus, with the user header type CameraFrameHeader, and the frame itself should be the raw pixel data.
   8) (for robots) {device_name}/{channel_name}/states/joint_position[type: a somehow variable-length array of doubles]: iceoryx2 channel publishing the joint position of the robot (if supported)
   9) (for robots) {device_name}/{channel_name}/states/joint_velocity[type: a somehow variable-length array of doubles]: iceoryx2 channel publishing the joint velocity of the robot (if supported)
   10) (for robots) {device_name}/{channel_name}/states/joint_effort[type: a somehow variable-length array of doubles]: iceoryx2 channel publishing the joint effort of the robot (if supported)
   11) (for robots) {device_name}/{channel_name}/states/end_effector_pose[type: a somehow variable-length array of doubles]: iceoryx2 channel publishing the end-effector pose of the robot (if supported)
   12) (for robots) {device_name}/{channel_name}/states/end_effector_twist[type: a somehow variable-length array of doubles]: iceoryx2 channel publishing the end-effector twist of the robot (if supported)
   13) (for robots) {device_name}/{channel_name}/states/end_effector_wrench[type: a somehow variable-length array of doubles]: iceoryx2 channel publishing the end-effector wrench of the robot (if supported)
   14) (for robots) {device_name}/{channel_name}/states/parallel_position[type: a somehow variable-length array of doubles]: iceoryx2 channel publishing the parallel position of the robot (if supported)
   15) (for robots) {device_name}/{channel_name}/states/parallel_velocity[type: a somehow variable-length array of doubles]: iceoryx2 channel publishing the parallel velocity of the robot (if supported)
   16) (for robots) {device_name}/{channel_name}/states/parallel_effort[type: a somehow variable-length array of doubles]: iceoryx2 channel publishing the parallel effort of the robot (if supported)
   17) (for robots) {device_name}/{channel_name}/commands/joint_mit[type: a somehow variable-length array of doubles]: iceoryx2 channel subscribing to the joint MIT command of the robot (if supported)
   18) (for robots) {device_name}/{channel_name}/commands/joint_pos[type: a somehow variable-length array of doubles]: iceoryx2 channel subscribing to the joint position command of the robot (if supported)
   19) (for robots) {device_name}/{channel_name}/commands/end_pose[type: a somehow variable-length array of doubles]: iceoryx2 channel subscribing to the end-effector pose command of the robot (if supported)
   20) (for robots) {device_name}/{channel_name}/commands/parallel_mit[type: a somehow variable-length array of doubles]: iceoryx2 channel subscribing to the parallel MIT command of the robot (if supported)
   21) (for robots) {device_name}/{channel_name}/commands/parallel_position[type: a somehow variable-length array of doubles]: iceoryx2 channel subscribing to the parallel position command of the robot (if supported)


