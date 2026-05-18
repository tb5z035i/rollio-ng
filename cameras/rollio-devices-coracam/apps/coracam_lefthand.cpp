#include "device_descriptor.hpp"
#include "device_main.hpp"

auto main(int argc, char* argv[]) -> int {
    return rollio::coracam::coracam_main(argc, argv, rollio::coracam::kLefthandDescriptor);
}
