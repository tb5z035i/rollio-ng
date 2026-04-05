#include <iostream>
#include <string>

#include "rollio/types.h"

int main(int argc, char* argv[]) {
    if (argc < 2) {
        std::cerr << "Usage: rollio-camera-pseudo <probe|validate|capabilities|run> [args...]\n";
        return 1;
    }

    std::string cmd = argv[1];
    if (cmd == "probe" || cmd == "validate" || cmd == "capabilities" || cmd == "run") {
        std::cout << "rollio-camera-pseudo: " << cmd << " stub\n";
        return 0;
    }

    std::cerr << "Unknown subcommand: " << cmd << "\n";
    return 1;
}
