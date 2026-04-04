## Key components:

### 1. Image Sensors 
Typical sensors are: realsense d435if, simple usb webcam, but the module should be extensible.

For any supported sensors, these probing methods should be provided: 
1) id probe (static method, find all sensor instances, give an id list)
2) validate (given identifier (ID?), validate the existance)
3) capability probe (basically parameters probe): given an id, gives the supported (width,height,fps) combinations, and the supported frame type (MJPG or YUYV in case of v4l2 usb cameras). And other params like the sensor ID, etc.
This probing part should be a fast executable that gives the results in compact structured output (optionally pretty formated for human), with a maximum latency of 200ms.

And for the actual frame fetching part, it should be another executable that the images should be fetched with given configurations and publish the raw frame to the given channel to the iceoryx2. This executable should also listen to iceoryx2 event for gracefully shutdown.

It should be noted chat multiple channels can be provided by the same image sensor. Also, since this is an IO intensive module, the cpu resources should be minimized. Async mechanism should be used to fetch the frames, while persisting low latency.

This module is preferred to be implemented in C++.

### 2. Robots
Typical robots are: AIRBOT play (examples and codes in legacy rollio), and in the future other robots like UR, etc.

The robots should provide methods to:

1) id probe (static method, find all robots with the same type, gives the id list)
2) validate (given identifier, can ID in the case of AIRBOT Play)
3) configuration probe: given an id, gives the device SN, and some category-specific parameters (like end effectors connected to the robot in case of the AIRBOT, etc.)
This probing part should also be a fast executable that gives the results in compact structured output (optionally pretty formated for human), with a maximum latency of 200ms.

And for the actual control part, each robot category should provide an executable that can be used to control the robot.
1) For all robots, controls are in free-drive / command-drive / planning mode for different use cases. Some robots (like UMI gripper) may only support some of the modes (free-drive in the case of UMI gripper). The difference between command-drive and planning mode is that command-drive is high-freq (at least 10Hz), and planning mode is when external command is sent only once.
2) Robots should publish the states (joint positions, velocities, efforts, etc.) to the iceoryx2, with as little latency as possible.
3) For robots that support command-drive and planning mode, the control executable should be able to receive commands from the iceoryx2, and execute them.

The robot arms are supposed to support task-space control (cartesian) and joint-space control. If task-space control is to be supported, an additional FK/IK layer is required.

If one robot is specified to follow another with the same type, the direct joint-space mapping should be used with best effort.

It should be noted that multiple "robots" can be provided by the same device (like AIRBOT Play and AIRBOT G2 (the gripper) / AIRBOT E2 (the demonstrator)) can have different components. For instance, AIRBOT Play and AIRBOT G2 may share the can0 interface, but it is theoretically possible that the arm (AIRBOT Play) is following one arm and the AIRBOT G2 is following another end effector.

It should be noted that, the high-freq polling (like what is done in AIRBOT Play examples) should not be exposed to the user. The user should be able to send commands to the robot, and the robot should execute them in real-time.

This module is preferred to be implemented in Python & C++, depending on the support of the robot vendor.

### 3. Encoder

This module is supposed to convert raw rgb (3-channel) and depth frames (1-channel) into h265/h264/av1 chunks.

This encoder module should also maintain a queue by itself. The size of the queue is configurable. If the queue is full (or near full), send a notifier to the controller.

This encoder module should provide a separate simple executable to probe the supported codecs and configurations.

Encoders that should be supported:
1) h265/h264/av1 (hardware acceleration is preferred, but software encoding is also supported)
2) ffv1 (for the depth frames, 1-channel and also lossless)
3) mjpeg (for the rgb frames, less preferred)

The actual encoding executable should be a fast executable that receives live raw images from iceoryx2 and send encoded chunks to the iceoryx2.

This module is preferred to be implemented in Rust.

### 4. Storage

This module is supposed to store the encoded chunks to storage backends.

For now, at least these three backends should be supported:
1) local file system
2) remote file system (like S3, etc.)
3) HTTP upload

The storage module should also maintain a queue by itself. The size of the queue is configurable. If the queue is full (or near full), send a notifier to the controller.

Specifically for HTTP upload backend, provide a simple HTTP server.

This module is preferred to be implemented in Rust.

### 5. Visualizer

This module is supposed to receive data (encoded chunks, live robot states, etc.) from the iceoryx2 and make them available for UI (specified later).

The video data should be transformed into rtsp stream for react-based UI to consume.
The robot states should be transformed into another friendly way (idk, maybe websocket?) for the react-based UI to consume.

This module should be robust, which means it is runable even without input data or client, and as soon as input data and client are present it would be normally working.

This module I think is better to be implemented in C++ / python. Maybe look for better solutions, in Rust maybe?

### 6. UI

A react project that:

1. Contains the components that would be visualized
2. UI layouts in the procedures described by the user story

In the user story, there are many UX interactions (like choices among options, and user-input), these should be unifomly implemented.

This module should be robust, which means it is runable even without data served from the backend, but it should be able to show the data when it is available.

The module should provide a single executable with cmd args, just like other modules, so that it can be naturally launched by the controller.

### 7. Controller

This controller is supposed to parse the configs and launch all other components as subprocesses. It owns all other modules, and manages the lifecycle of all other modules. All other modules should be able to send events to the controller, and the controller should be able to send events to all other modules. All modules, including the controller itself, should be able to be shutdown gracefully.

This module is preferred to be implemented in Rust.

## Technical choices:

1. The rendering and layout should be designed and implemented in **react** but render in TUI. A small validation project seems to prove that **ink** is a good choice: @external/react-tui . This would mean that, all components that support previewing (image preview, robot state bars, etc.), should have corresponding UI parts. The UI should be designed to be responsive and adaptive to different screen sizes.

2. The whole framework should work in a multi-process manner. The communication between the modules should be implemented by iceoryx2, unless otherwise specified.

3. Unit tests are important, and should be implemented for all modules. Mock backends should be provided for all modules to test the functionality without the actual dependencies.

