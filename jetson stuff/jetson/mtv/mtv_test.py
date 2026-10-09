import os
os.environ['JETSON_MODEL_NAME'] = "JETSON_ORIN_NANO"

import time
from mtv_module import MTV
from procedure_io import shutdown


def main():
    mtv = None
    try:
        mtv = MTV(15, 33)
        for angle in (0, 90, 0):
            mtv.command(angle)
            time.sleep(5)
    finally:
        shutdown(mtv)


if __name__ == "__main__":
    main()
