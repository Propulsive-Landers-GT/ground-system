import os
os.environ['JETSON_MODEL_NAME'] = "JETSON_ORIN_NANO"

import Jetson.GPIO as GPIO
from mtv_module import Angle_Profile, MTV_Profile
from procedure import Procedure, get_angle_profile

def main():
    angle_profile_start = 1.2 +0.5
    angle_profile_length, angles, angle_profile_times = get_angle_profile("100-2.8", angle_profile_start)
    angle_profile_end = angle_profile_length + angle_profile_start

    angle_profile = Angle_Profile(angles, angle_profile_times)

    commands = ["omv open", "omv open", "igv open", "kaboom start",
                "igv close", "omv close", "puiso open", "pumv open", "puiso close", "pumv close", "pumv close"]
    mtv_profile_times = [0, 0.1, 1.0, 1.2,
                         angle_profile_end, angle_profile_end + 1.0, angle_profile_end + 1.1, angle_profile_end + 1.2, angle_profile_end + 3.2, angle_profile_end + 4.2, angle_profile_end + 4.3]

    mtv_profile = MTV_Profile(commands, mtv_profile_times)

    procedure = Procedure(angle_profile, mtv_profile, 15, 33, 0, debug=True)
    procedure.run()

if __name__ == "__main__":
    main()