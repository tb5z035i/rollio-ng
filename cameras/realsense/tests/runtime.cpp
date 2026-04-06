#include <array>
#include <chrono>
#include <cstdio>
#include <cstdlib>
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

auto spawn_run_command(const std::string& config_inline) -> pid_t {
    const auto pid = fork();
    if (pid < 0) {
        throw std::runtime_error("fork failed");
    }
    if (pid == 0) {
        char* argv[] = {
            const_cast<char*>(ROLLIO_CAMERA_REALSENSE_BIN),
            const_cast<char*>("run"),
            const_cast<char*>("--config-inline"),
            const_cast<char*>(config_inline.c_str()),
            nullptr,
        };
        execv(ROLLIO_CAMERA_REALSENSE_BIN, argv);
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
        "type = \"camera\"\n"
        "driver = \"realsense\"\n"
        "id = \"invalid_serial\"\n"
        "width = 640\n"
        "height = 480\n"
        "fps = 30\n"
        "pixel_format = \"rgb24\"\n"
        "stream = \"color\"\n"
        "transport = \"usb\"\n";
    const auto pid = spawn_run_command(config_inline);
    wait_for_failure(pid, std::chrono::seconds(2));
}

} // namespace

auto main() -> int {
    try {
        run_probe_test();
        run_invalid_capabilities_test();
        run_invalid_runtime_test();
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "rollio-camera-realsense-tests: " << error.what() << '\n';
        return 1;
    }
}
