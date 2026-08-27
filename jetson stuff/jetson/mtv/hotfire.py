import os
os.environ['JETSON_MODEL_NAME'] = "JETSON_ORIN_NANO"

import Jetson.GPIO as GPIO
from mtv_module import Angle_Profile, PFS_Profile
from procedure import Procedure, get_angle_profile

def main():
    angle_profile_start = 53.6
    # angle_profile_length, angle_commands = get_angle_profile("hold-15-0.25 ramp-15-100-1.25 hold-100-4.5", angle_profile_start)
    angle_profile_length, angle_commands = get_angle_profile("hold-20-8", angle_profile_start)
    angle_profile_end = angle_profile_length + angle_profile_start

    angle_profile = Angle_Profile(angle_commands)

    # commands = ["oiso close", "oiso close",  "engine begin", "nitrous begin", "lfvent open", "lfvent close", "puiso open", "pufill open", "omv open", "igv open", "kaboom start", "kaboom end", "igv close", "ofill open",
    #             "ofill close", "mtv open", "engine end", "nitrous end", "nitrous end"]
    # mtv_profile_times = [0, 0.1, 0.5, 1, 21, 22, 22.5, 23, 28, 28.5, 28.7, 29.7, 33.5, 33.6,
    #                      angle_profile_end, angle_profile_end + 0.5, angle_profile_end + 2.0, angle_profile_end + 2.1, angle_profile_end + 2.2]

    pfs_commands = [("oiso close", 0), ("oiso close", 0.1), ("engine begin", 0.5), ("nitrous begin", 1),
                ("lfvent open", 21), ("lfvent close", 22), ("puiso open", 22.5), ("pufill open", 23),
                ("omv open", 48), ("igv open", 48.5), ("kaboom start", 48.7), ("kaboom end", 49.7), ("igv close", 53.5), ("ofill open", 53.6), # angle profile start here
                ("ofill close", angle_profile_end), ("mtv open", angle_profile_end + 0.5), ("engine end", angle_profile_end + 2.0), ("nitrous end", angle_profile_end + 2.1), ("nitrous end", angle_profile_end + 2.2)]

    pfs_profile = PFS_Profile(pfs_commands)

    procedure = Procedure(angle_profile, pfs_profile, 15, 33, 0, debug=True)
    procedure.run()

if __name__ == "__main__":
    main()