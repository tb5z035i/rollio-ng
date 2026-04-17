#include <array>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <csignal>
#include <iostream>
#include <stdexcept>
#include <string>
#include <thread>

#include <sys/wait.h>
#include <unistd.h>

namespace {

using SteadyClock = std::chrono::steady_clock;

auto capture_command_output(const std::string& command) -> std::string {
    std::array<char, 256> buffer {};
    std::string output;

    auto* pipe = popen(command.c_str(), "r");
    if (pipe == nullptr) {
        throw std::runtime_error("failed to execute command");
    }

    while (fgets(buffer.data(), static_cast<int>(buffer.size()), pipe) != nullptr) {
        output += buffer.data();
    }

    const auto status = pclose(pipe);
    if (status != 0) {
        throw std::runtime_error("command failed unexpectedly");
    }

    return output;
}

auto spawn_run_command(const std::string& config_inline, bool dry_run) -> pid_t {
    const auto pid = fork();
    if (pid < 0) {
        throw std::runtime_error("fork failed");
    }
    if (pid == 0) {
        if (dry_run) {
            char* argv[] = {
                const_cast<char*>(ROLLIO_CAMERA_REALSENSE_BIN),
                const_cast<char*>("run"),
                const_cast<char*>("--dry-run"),
                const_cast<char*>("--config-inline"),
                const_cast<char*>(config_inline.c_str()),
                nullptr,
            };
            execv(ROLLIO_CAMERA_REALSENSE_BIN, argv);
        } else {
            char* argv[] = {
                const_cast<char*>(ROLLIO_CAMERA_REALSENSE_BIN),
                const_cast<char*>("run"),
                const_cast<char*>("--config-inline"),
                const_cast<char*>(config_inline.c_str()),
                nullptr,
            };
            execv(ROLLIO_CAMERA_REALSENSE_BIN, argv);
        }
        _exit(127);
    }

    return pid;
}

auto wait_for_failure(const pid_t pid, const std::chrono::seconds timeout) -> void {
    const auto deadline = SteadyClock::now() + timeout;
    int status = 0;

    while (SteadyClock::now() < deadline) {
        const auto result = waitpid(pid, &status, WNOHANG);
        if (result == pid) {
            if (WIFEXITED(status) && WEXITSTATUS(status) != 0) {
                return;
            }
            throw std::runtime_error("realsense run command unexpectedly succeeded");
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
    }

    kill(pid, SIGKILL);
    throw std::runtime_error("realsense run command did not fail within the timeout");
}

auto wait_for_success(const pid_t pid, const std::chrono::seconds timeout) -> void {
    const auto deadline = SteadyClock::now() + timeout;
    int status = 0;

    while (SteadyClock::now() < deadline) {
        const auto result = waitpid(pid, &status, WNOHANG);
        if (result == pid) {
            if (WIFEXITED(status) && WEXITSTATUS(status) == 0) {
                return;
            }
            throw std::runtime_error(
                "realsense run command exited with non-zero status: " + std::to_string(status)
            );
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
    }

    kill(pid, SIGKILL);
    throw std::runtime_error("realsense run command did not finish within the timeout");
}

auto run_probe_test() -> void {
    const auto command = std::string("\"") + ROLLIO_CAMERA_REALSENSE_BIN + "\" probe";
    const auto output = capture_command_output(command);
    if (output.find('[') == std::string::npos || output.find(']') == std::string::npos) {
        throw std::runtime_error("probe output should be a JSON array");
    }
}

auto run_invalid_capabilities_test() -> void {
    const auto command = std::string("\"") + ROLLIO_CAMERA_REALSENSE_BIN + "\" capabilities invalid_serial >/dev/null 2>&1";
    const auto status = std::system(command.c_str());
    if (status == 0) {
        throw std::runtime_error("capabilities unexpectedly succeeded for an invalid serial");
    }
}

auto run_invalid_runtime_test() -> void {
    const auto config_inline =
        "name = \"realsense_invalid\"\n"
        "driver = \"realsense\"\n"
        "id = \"invalid_serial\"\n"
        "bus_root = \"device/realsense_invalid\"\n"
        "\n"
        "[[channels]]\n"
        "channel_type = \"color\"\n"
        "kind = \"camera\"\n"
        "enabled = true\n"
        "profile = { width = 640, height = 480, fps = 30, pixel_format = \"rgb24\" }\n";
    const auto pid = spawn_run_command(config_inline, false);
    wait_for_failure(pid, std::chrono::seconds(2));
}

// Regression: when the controller serializes a `BinaryDeviceConfig` via
// `toml::to_string`, the Rust `toml` crate emits each channel's `profile`
// and `command_defaults` as nested `[channels.profile]` /
// `[channels.command_defaults]` table headers — not as inline tables. The
// hand-rolled C++ parser used to choke on these headers and the realsense
// driver would exit with status 1, which surfaced in the wizard as
// "child \"device-realsense_rgb\" exited with status exit status: 1".
auto run_serialized_runtime_dry_run_test() -> void {
    const auto config_inline =
        "name = \"realsense_rgb\"\n"
        "executable = \"rollio-camera-realsense\"\n"
        "driver = \"realsense\"\n"
        "id = \"332322071743\"\n"
        "bus_root = \"realsense_rgb\"\n"
        "\n"
        "[[channels]]\n"
        "channel_type = \"color\"\n"
        "kind = \"camera\"\n"
        "enabled = true\n"
        "name = \"realsense_rgb\"\n"
        "channel_label = \"Intel RealSense RGB\"\n"
        "publish_states = []\n"
        "recorded_states = []\n"
        "\n"
        "[channels.profile]\n"
        "width = 1920\n"
        "height = 1080\n"
        "fps = 30\n"
        "pixel_format = \"rgb24\"\n"
        "\n"
        "[channels.command_defaults]\n"
        "joint_mit_kp = []\n"
        "joint_mit_kd = []\n"
        "parallel_mit_kp = []\n"
        "parallel_mit_kd = []\n";
    const auto pid = spawn_run_command(config_inline, true);
    wait_for_success(pid, std::chrono::seconds(2));
}

} // namespace

auto main() -> int {
    try {
        run_probe_test();
        run_invalid_capabilities_test();
        run_invalid_runtime_test();
        run_serialized_runtime_dry_run_test();
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "rollio-camera-realsense-tests: " << error.what() << '\n';
        return 1;
    }
}
