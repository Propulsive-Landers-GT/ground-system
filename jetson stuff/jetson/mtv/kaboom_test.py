import os
os.environ['JETSON_MODEL_NAME'] = "JETSON_ORIN_NANO"

import Jetson.GPIO as GPIO
from mtv_module import Angle_Profile, PFS_Profile
from procedure import Procedure, get_angle_profile

def main():
    angle_profile_start = 1.2 +0.5
    angle_profile_length, angle_commands = get_angle_profile("hold-100-2.8", angle_profile_start)
    angle_profile_end = angle_profile_length + angle_profile_start

    angle_profile = Angle_Profile(angle_commands)

    commands = ["omv open", "omv open", "igv open", "kaboom start",
                "igv close", "omv close", "puiso open", "pumv open", "puiso close", "pumv close", "pumv close"]
    mtv_profile_times = [0, 0.1, 1.0, 1.2,
                         angle_profile_end, angle_profile_end + 1.0, angle_profile_end + 1.1, angle_profile_end + 1.2, angle_profile_end + 3.2, angle_profile_end + 4.2, angle_profile_end + 4.3]

    mtv_profile = PFS_Profile(list(zip(commands, mtv_profile_times)))

    procedure = Procedure(angle_profile, mtv_profile, 15, 33, debug=True)
    procedure.run()

if __name__ == "__main__":
    main()
