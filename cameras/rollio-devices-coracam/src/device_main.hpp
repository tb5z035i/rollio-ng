#ifndef ROLLIO_DEVICES_CORACAM_DEVICE_MAIN_HPP
#define ROLLIO_DEVICES_CORACAM_DEVICE_MAIN_HPP

#include "device_descriptor.hpp"

namespace rollio::coracam {

// Entry point shared by all three coracam executables. Parses argv and
// dispatches to probe/validate/query/run handlers. Returns a process exit code.
int coracam_main(int argc, char* argv[], const DeviceDescriptor& desc);

}  // namespace rollio::coracam

#endif  // ROLLIO_DEVICES_CORACAM_DEVICE_MAIN_HPP
