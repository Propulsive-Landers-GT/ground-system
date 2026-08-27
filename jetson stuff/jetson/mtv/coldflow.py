import os
os.environ['JETSON_MODEL_NAME'] = "JETSON_ORIN_NANO"

import Jetson.GPIO as GPIO
from mtv_module import Angle_Profile, MTV_Profile
from procedure import Procedure, get_angle_profile

def main():
    angle_profile_start = 0.5
    angle_profile_length, angles, angle_profile_times = get_angle_profile("100-8", angle_profile_start)
    angle_profile_end = angle_profile_length + angle_profile_start

    angle_profile = Angle_Profile(angles, angle_profile_times)

    commands = ["ofill open", "ofill open", "engine begin", "nitrous begin", "omv open",
                "ofill close", "mtv open", "engine end", "nitrous end", "nitrous end"]
    mtv_profile_times = [0, 0.1, 0.2, 0.3, 0.5,
                         angle_profile_end, angle_profile_end + 0.5, angle_profile_end + 1.9, angle_profile_end + 2.0, angle_profile_end + 2.1]

    mtv_profile = MTV_Profile(commands, mtv_profile_times)

    procedure = Procedure(angle_profile, mtv_profile, 15, 33, 0, debug=True)
    procedure.run()

if __name__ == "__main__":
    main()