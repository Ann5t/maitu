import json
import fudian_example

print(json.dumps({"version": fudian_example.VERSION, "rendered": fudian_example.render("sample")}))
