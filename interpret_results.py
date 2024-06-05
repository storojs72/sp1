import sys
import os
import re


results_file = sys.argv[1]
if not os.path.exists(results_file):
    print("experiment results file is missing")
    exit(1)

with open(os.path.basename(results_file)) as file:
    lines = [line.rstrip() for line in file]

def chunks(lst, n):
    """Yield successive n-sized chunks from lst."""
    for i in range(0, len(lst), n):
        yield lst[i:i + n]


results = []
list.append(results, lines[0])
list.append(results, lines[1])
list.append(results, lines[2])

input_results = list(chunks(lines[3:], 17))

prove_core_min = float("inf")
compress_min = float("inf")
for item in input_results:
    output = [x for x in item if x != '']
    #print(output)

    n = [float(s) for s in re.findall(r'[\d]*[.][\d]+', str(output))]
    prove_core_average = 0
    compress_average = 0
    for idx, value in enumerate(n):
        if idx%2==0:
            prove_core_average = prove_core_average + value
        else:
            compress_average = compress_average + value

    prove_core_average = prove_core_average / (len(n) / 2)
    compress_average = compress_average / (len(n) / 2)
    list.append(results, '[sha-extend average results] prove_core took: {0:2f} compress took: {1:2f} | {2:s}, {3:s}, {4:s}, {5:s} |'.format(prove_core_average, compress_average, output[0], output[1], output[2], output[3]))
    if prove_core_average < prove_core_min:
        prove_core_min = prove_core_average

    if compress_average < compress_min:
        compress_min = compress_average


with open('average.txt', 'w') as f:
    for line in results:
        f.write(f"{line}\n")
    f.write("\n")
    f.write("[min values] prove_core: {0:2f}\n".format(prove_core_min))
    f.write("[min values] compress: {0:2f}\n".format(compress_min))
