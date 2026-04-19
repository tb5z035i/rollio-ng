#include "iox2/iceoryx2.hpp"
#include "rollio/topic_names.hpp"
#include "rollio/types.h"

#include <chrono>
#include <array>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>

#include <sys/wait.h>
#include <unistd.h>

namespace {

using IpcNode = iox2::Node<iox2::ServiceType::Ipc>;
using FrameSubscriber =
    iox2::Subscriber<iox2::ServiceType::Ipc, iox2::bb::Slice<uint8_t>, rollio::CameraFrameHeader>;
using ControlPublisher = iox2::Publisher<iox2::ServiceType::Ipc, rollio::ControlEvent, void>;
using SteadyClock = std::chrono::steady_clock;

struct TestPorts {
    IpcNode node;
    FrameSubscriber frame_subscriber;
    ControlPublisher control_publisher;
};

struct FrameObservation {
    rollio::CameraFrameHeader header;
    uint64_t payload_size;
};

auto count_substring(const std::string& text, const std::string& needle) -> std::size_t {
    auto count = std::size_t {0};
    auto pos = std::string::size_type {0};
    while ((pos = text.find(needle, pos)) != std::string::npos) {
        ++count;
        pos += needle.size();
    }
    return count;
}

auto capture_stdout(const std::string& command) -> std::string {
    std::array<char, 256> buffer {};
    std::string output;

    auto* pipe = popen(command.c_str(), "r");
    if (pipe == nullptr) {
        throw std::runtime_error("failed to execute command: " + command);
    }

    while (fgets(buffer.data(), static_cast<int>(buffer.size()), pipe) != nullptr) {
        output += buffer.data();
    }

    const auto status = pclose(pipe);
    if (status != 0) {
        throw std::runtime_error("command failed: " + command);
    }

    return output;
}

auto unique_name() -> std::string {
    const auto nanos = std::chrono::duration_cast<std::chrono::nanoseconds>(
                           std::chrono::system_clock::now().time_since_epoch()
                       )
                           .count();
    return "pseudo_cam_test_" + std::to_string(nanos);
}

auto create_test_ports(const std::string& device_name) -> TestPorts {
    using namespace iox2;

    auto node = NodeBuilder().create<ServiceType::Ipc>().value();

    const auto frame_service_name = ServiceName::create(rollio::camera_frames_service_name(device_name).c_str()).value();
    auto frame_service = node.service_builder(frame_service_name)
                             .publish_subscribe<bb::Slice<uint8_t>>()
                             .user_header<rollio::CameraFrameHeader>()
                             .open_or_create()
                             .value();
    auto frame_subscriber = frame_service.subscriber_builder().create().value();

    const auto control_service_name = ServiceName::create(rollio::CONTROL_EVENTS_SERVICE).value();
    auto control_service = node.service_builder(control_service_name)
                               .publish_subscribe<rollio::ControlEvent>()
                               .open_or_create()
                               .value();
    auto control_publisher = control_service.publisher_builder().create().value();

    return TestPorts {
        std::move(node),
        std::move(frame_subscriber),
        std::move(control_publisher),
    };
}

auto spawn_camera_process(const std::string& config_inline) -> pid_t {
    const auto pid = fork();
    if (pid < 0) {
        throw std::runtime_error("fork failed");
    }
    if (pid == 0) {
        char* argv[] = {
            const_cast<char*>(ROLLIO_DEVICE_PSEUDO_CAMERA_BIN),
            const_cast<char*>("run"),
            const_cast<char*>("--config-inline"),
            const_cast<char*>(config_inline.c_str()),
            nullptr,
        };
        execv(ROLLIO_DEVICE_PSEUDO_CAMERA_BIN, argv);
        _exit(127);
    }

    return pid;
}

auto collect_frames(FrameSubscriber& subscriber, std::size_t count, const std::chrono::seconds timeout)
    -> std::vector<FrameObservation> {
    auto frames = std::vector<FrameObservation> {};
    const auto deadline = SteadyClock::now() + timeout;

    while (SteadyClock::now() < deadline && frames.size() < count) {
        auto sample = subscriber.receive().value();
        if (sample.has_value()) {
            frames.push_back(FrameObservation {
                sample->user_header(),
                sample->payload().number_of_bytes(),
            });
        } else {
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
    }

    if (frames.size() < count) {
        throw std::runtime_error("did not receive enough frames");
    }

    return frames;
}

auto send_shutdown(ControlPublisher& publisher) -> void {
    rollio::ControlEvent event {};
    event.tag = rollio::ControlEventTag::Shutdown;
    publisher.send_copy(event).value();
}

auto wait_for_exit(const pid_t pid, const std::chrono::seconds timeout) -> void {
    const auto deadline = SteadyClock::now() + timeout;
    int status = 0;

    while (SteadyClock::now() < deadline) {
        const auto result = waitpid(pid, &status, WNOHANG);
        if (result == pid) {
            if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
                throw std::runtime_error("camera process exited unsuccessfully");
            }
            return;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
    }

    kill(pid, SIGKILL);
    throw std::runtime_error("camera process did not exit after shutdown");
}

auto run_probe_test() -> void {
    const auto command = std::string("\"") + ROLLIO_DEVICE_PSEUDO_CAMERA_BIN + "\" probe --count 3";
    const auto output = capture_stdout(command);
    if (count_substring(output, "\"id\":\"pseudo_cam_") != 3U) {
        throw std::runtime_error("probe output did not contain three pseudo camera ids");
    }
}

auto run_capabilities_test() -> void {
    const auto command = std::string("\"") + ROLLIO_DEVICE_PSEUDO_CAMERA_BIN + "\" capabilities pseudo_cam_0";
    const auto output = capture_stdout(command);
    if (output.find("\"rgb24\"") == std::string::npos || output.find("\"width\":640") == std::string::npos) {
        throw std::runtime_error("capabilities output is missing expected profile data");
    }
}

auto run_runtime_test() -> void {
    const auto device_name = unique_name();
    auto ports = create_test_ports(device_name);

    const auto config_inline =
        "name = \"" + device_name + "\"\n"
        "type = \"camera\"\n"
        "driver = \"pseudo\"\n"
        "id = \"" + device_name + "_id\"\n"
        "width = 320\n"
        "height = 240\n"
        "fps = 20\n"
        "pixel_format = \"rgb24\"\n"
        "stream = \"color\"\n"
        "transport = \"simulated\"\n";

    const auto pid = spawn_camera_process(config_inline);
    const auto frames = collect_frames(ports.frame_subscriber, 12U, std::chrono::seconds(3));

    if (frames.front().header.width != 320U || frames.front().header.height != 240U) {
        throw std::runtime_error("frame header dimensions are incorrect");
    }
    if (frames.front().payload_size != 320U * 240U * 3U) {
        throw std::runtime_error("frame payload size is incorrect");
    }

    for (std::size_t idx = 1; idx < frames.size(); ++idx) {
        if (frames[idx - 1].header.frame_index >= frames[idx].header.frame_index) {
            throw std::runtime_error("frame indices are not strictly increasing");
        }
        if (frames[idx - 1].header.timestamp_ns >= frames[idx].header.timestamp_ns) {
            throw std::runtime_error("frame timestamps are not strictly increasing");
        }
    }

    send_shutdown(ports.control_publisher);
    wait_for_exit(pid, std::chrono::seconds(2));
}

} // namespace

auto main() -> int {
    try {
        run_probe_test();
        run_capabilities_test();
        run_runtime_test();
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "rollio-device-pseudo-camera-tests: " << error.what() << '\n';
        return 1;
    }
}
