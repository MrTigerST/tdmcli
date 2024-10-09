from concurrent.futures import ThreadPoolExecutor, as_completed
import os
import sys
import shutil
import requests
from pathlib import Path
from tqdm import tqdm
import time

VERSION = "1.0"
KEY = "tdmcliKeyy"
MAX_WORKERS = os.cpu_count() * 4

def xor_crypt(data, key):
    key_bytes = key.encode()
    key_len = len(key_bytes)
    return bytearray([b ^ key_bytes[i % key_len] for i, b in enumerate(data)])

def get_executable_dir():
    return os.path.dirname(os.path.realpath(sys.argv[0]))

def process_file(file_path, root_dir):
    relative_path = os.path.relpath(file_path, root_dir)
    with open(file_path, 'rb') as input_file:
        content = input_file.read()
    encrypted_content = xor_crypt(content, KEY)
    return relative_path, encrypted_content

def create_template(template_name, root_dir='.'):
    print(f"Loading... Creating template '{template_name}'.")
    time.sleep(0.5)

    template_path = os.path.join(get_executable_dir(), f"{template_name}.tdmcli")
    all_files = [os.path.join(root, file) for root, _, files in os.walk(root_dir) for file in files]

    with ThreadPoolExecutor(max_workers=MAX_WORKERS) as executor:
        futures = [executor.submit(process_file, file_path, root_dir) for file_path in all_files]
        
        with open(template_path, 'wb') as template_file:
            for future in tqdm(as_completed(futures), total=len(futures), desc="Creating Template"):
                relative_path, encrypted_content = future.result()
                template_file.write(f"FILE: {relative_path}\n".encode())
                template_file.write(f"SIZE: {len(encrypted_content)}\n".encode())
                template_file.write(encrypted_content)
                template_file.write(b"\nEND_OF_FILE\n")
    
    print(f"Template '{template_name}' created successfully.")

def process_template_file(lines, start_index):
    file_name = lines[start_index][6:].strip().decode()
    size = int(lines[start_index + 1][6:].strip())
    encrypted_content = b''.join(lines[start_index + 2:])
    decrypted_content = xor_crypt(encrypted_content[:size], KEY)

    file_dir = os.path.dirname(file_name)
    if file_dir and not os.path.exists(file_dir):
        os.makedirs(file_dir, exist_ok=True)

    with open(file_name, 'wb') as output_file:
        output_file.write(decrypted_content)

def apply_template(template_name):
    print(f"Loading... Applying template '{template_name}'.")
    time.sleep(0.5)

    template_path = os.path.join(get_executable_dir(), f"{template_name}.tdmcli")
    if not os.path.exists(template_path):
        print(f"Template '{template_name}' not found.")
        return

    with open(template_path, 'rb') as template_file:
        lines = template_file.readlines()

    futures = []
    with ThreadPoolExecutor(max_workers=MAX_WORKERS) as executor:
        total_files = sum(1 for line in lines if line.startswith(b"FILE: "))
        i = 0
        with tqdm(total=total_files, desc="Applying Template") as pbar:
            while i < len(lines):
                if lines[i].startswith(b"FILE: "):
                    futures.append(executor.submit(process_template_file, lines, i))
                    i += 3
                    pbar.update(1)
                else:
                    i += 1

        for future in as_completed(futures):
            future.result()

    print(f"Template '{template_name}' applied successfully.")

def delete_template(template_name):
    template_path = os.path.join(get_executable_dir(), f"{template_name}.tdmcli")
    if os.path.exists(template_path):
        os.remove(template_path)
        print(f"Template '{template_name}' deleted.")
    else:
        print(f"Template '{template_name}' not found.")

def list_templates():
    templates = [f.stem for f in Path(get_executable_dir()).glob("*.tdmcli")]
    if templates:
        print("Your templates:\n\n")
        for t in templates:
            print(t)
    else:
        print("No templates found.")

def actual_version():
    print(f"Version of tdmcli: {VERSION}")

def get_latest_release_version():
    try:
        response = requests.get("https://raw.githubusercontent.com/MrTigerST/tdmcli/main/version")
        return response.text.strip()
    except requests.RequestException:
        return None

def check_for_updates():
    latest_version = get_latest_release_version()
    if latest_version:
        print(f"Latest version available: {latest_version}")
        print(f"Your current version: {VERSION}")
        if latest_version != VERSION:
            print("A new version is available! Download it from GitHub.")
        else:
            print("You are using the latest version.")
    else:
        print("Failed to check for updates.")

def export_template(template_name, output_dir):
    template_path = os.path.join(get_executable_dir(), f"{template_name}.tdmcli")
    if os.path.exists(template_path):
        if not os.path.exists(output_dir):
            os.makedirs(output_dir)
        shutil.copy(template_path, os.path.join(output_dir, f"{template_name}.tdmcli"))
        print(f"Template '{template_name}' exported to '{output_dir}'")
    else:
        print(f"Template '{template_name}' not found.")

def import_template(input_file, template_name=None):
    if not template_name:
        template_name = Path(input_file).stem
    dest_path = os.path.join(get_executable_dir(), f"{template_name}.tdmcli")
    shutil.copy(input_file, dest_path)
    print(f"Template imported from '{input_file}' as '{template_name}'")

def show_help_command():
    help_text = """
Usage: tdmcli <command> <template_name | command_argument>

Examples:
tdmcli create <template_name>    Create a template.
tdmcli get <template_name>       Apply the template.
tdmcli delete <template_name>    Delete a template.
tdmcli list                     Show all templates.
tdmcli import <input_file> [template_name]      Import an external template.
tdmcli export <template_name> <output_dir> Export template.
tdmcli -v                       Show the current version.
tdmcli -u                       Check for updates.
tdmcli help                     Show this help.
"""
    print(help_text)

def main():
    if len(sys.argv) < 2:
        show_help_command()
        return

    command = sys.argv[1]

    if os.path.isfile(command):
        print(f"Detected file '{command}', importing as template.")
        import_template(command)
        return

    if command == "create" and len(sys.argv) == 3:
        create_template(sys.argv[2])
    elif command == "get" and len(sys.argv) == 3:
        apply_template(sys.argv[2])
    elif command == "delete" and len(sys.argv) == 3:
        delete_template(sys.argv[2])
    elif command == "list":
        list_templates()
    elif command == "-v":
        actual_version()
    elif command == "-u":
        check_for_updates()
    elif command == "export" and len(sys.argv) == 4:
        export_template(sys.argv[2], sys.argv[3])
    elif command == "import" and len(sys.argv) >= 3:
        import_template(sys.argv[2], sys.argv[3] if len(sys.argv) == 4 else None)
    else:
        show_help_command()

if __name__ == "__main__":
    main()

