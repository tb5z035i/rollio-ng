// SPDX-License-Identifier: Apache-2.0
//
// Smoke test for rollio-device-umi: verify probe + dry-run + validate
// emit the expected output.

#include <array>
#include <cstdio>
#include <cstdlib>
#include <iostream>
#include <string>

namespace {

#ifndef ROLLIO_DEVICE_UMI_BIN
#error "ROLLIO_DEVICE_UMI_BIN must be defined to the executable path"
#endif

int run_command(const std::string& cmd, std::string& output) {
    std::array<char, 4096> buffer{};
    output.clear();
    auto* pipe = popen(cmd.c_str(), "r");
    if (!pipe) {
        std::cerr << "popen failed for: " << cmd << "\n";
        return -1;
    }
    while (auto* line = fgets(buffer.data(), static_cast<int>(buffer.size()), pipe)) {
        output += line;
    }
    return pclose(pipe);
}

bool contains(const std::string& haystack, const std::string& needle) {
    return haystack.find(needle) != std::string::npos;
}

}  // namespace

int main() {
    std::string output;
    int rc;

    // probe
    rc = run_command(std::string(ROLLIO_DEVICE_UMI_BIN) + " probe --json", output);
    if (rc != 0 || !contains(output, "\"umi\"")) {
        std::cerr << "probe failed: rc=" << rc << " output=" << output << "\n";
        return 1;
    }

    // validate
    rc = run_command(std::string(ROLLIO_DEVICE_UMI_BIN) + " validate --json umi", output);
    if (rc != 0 || !contains(output, "\"valid\":true")) {
        std::cerr << "validate failed: rc=" << rc << " output=" << output << "\n";
        return 1;
    }

    // dry-run with an inline config
    const std::string toml = R"(name = "umi"
driver = "umi"
id = "umi"
bus_root = "umi"

[[channels]]
channel_type = "head_left"
kind = "camera"
enabled = true
profile = { width = 1280, height = 1088, fps = 20, pixel_format = "h264" }
dds_topic = "rt/robot/camera/head/left/video_encoded"

[[channels]]
channel_type = "imu_head"
kind = "imu"
enabled = true
dds_topic = "rt/robot/imu/head/data"
)";
    // Use printf-quote-wrapping to pass through shell.
    std::string cmd = std::string(ROLLIO_DEVICE_UMI_BIN) + " run --config-inline '" + toml +
                      "' --dry-run 2>&1";
    rc = run_command(cmd, output);
    if (rc != 0 || !contains(output, "dry-run ok bus_root=umi cameras=1 imus=1")) {
        std::cerr << "dry-run failed: rc=" << rc << " output=" << output << "\n";
        return 1;
    }

    return 0;
}
