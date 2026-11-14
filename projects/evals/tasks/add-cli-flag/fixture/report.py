import sys

def counts(path):
    with open(path) as file:
        lines = file.read().splitlines()
    return {"lines": len(lines), "words": sum(len(line.split()) for line in lines)}

def main():
    result = counts(sys.argv[1])
    for key, value in result.items():
        print(f"{key}: {value}")

if __name__ == "__main__":
    main()
