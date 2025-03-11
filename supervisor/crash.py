import time


def crash():
    time.sleep(10)
    raise Exception("Crash")


if __name__ == "__main__":
    crash()
