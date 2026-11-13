import argparse

def main(argv=None):
    p = argparse.ArgumentParser()
    p.add_argument('message')
    args = p.parse_args(argv)
    print(args.message)

if __name__ == '__main__':
    main()
