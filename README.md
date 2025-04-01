# ManhwaOCR
an app that lets you MTL your manhwa with ease. designed with simplicity at its core .

## Table of Contents
- [ManhwaOCR](#manhwaocr)
  - [Table of Contents](#table-of-contents)
- [Workflow](#workflow)
  - [Create Project](#create-project)
  - [Start the OCR](#start-the-ocr)
  - [Configure and Translate with Gemini API (Free!)](#configure-and-translate-with-gemini-api-free)
  - [Apply Translation and Save Manhwa](#apply-translation-and-save-manhwa)
- [Installation Guide](#installation-guide)
  - [Prerequisites](#prerequisites)
  - [Step 1: Clone the Repository](#step-1-clone-the-repository)
  - [Step 2: Install Required Dependencies](#step-2-install-required-dependencies)
  - [Step 3: Run the Application](#step-3-run-the-application)
  - [Troubleshooting](#troubleshooting)
    - [1. Some Text is Not Detected](#1-some-text-is-not-detected)
    - [2. EasyOCR Does Not Work as Intended](#2-easyocr-does-not-work-as-intended)
    - [3. PyQt5 Installation Issues](#3-pyqt5-installation-issues)
    - [4. Missing Dependencies](#4-missing-dependencies)
  - [Notes](#notes)
  - [Contributing](#contributing)

---

# Workflow

## Create Project

https://github.com/user-attachments/assets/29a9de14-5e11-4292-8943-4ad793abb5d3

## Start the OCR

https://github.com/user-attachments/assets/432fa7f7-9172-43b7-97c7-28e8d66b6758

## Configure and Translate with Gemini API (Free!)

yes, gemini api is free. if you still haven't made one, what are you waiting for!?!

https://github.com/user-attachments/assets/2b255b1e-a036-4b98-b22c-5d72854a65c3

## Apply Translation and Save Manhwa

https://github.com/user-attachments/assets/a3269eb7-2849-4a44-840b-c5433d3ce8fc



---

# Installation Guide

This guide will help you set up and run the Manhwa OCR Tool on your system. Follow the steps below to install all necessary dependencies and execute the application.

## Prerequisites

Before proceeding, ensure that you have the following installed on your system:

- **Python 3.9 or higher**: You can download it from [python.org](https://www.python.org/downloads/).
- **pip**: Python's package installer (usually included with Python).

Verify your Python and pip installations by running the following commands in your terminal:

```bash
python --version
pip --version
```

If these commands return version numbers, you're good to proceed.

---

## Step 1: Clone the Repository

Clone this repository to your local machine using the following command:

```bash
git clone https://github.com/Liiesl/ManhwaOCR.git
cd ManhwaOCR
```

## Step 2: Install Required Dependencies

Install core dependencies with a single command:
```bash
pip install PyQt5 qtawesome easyocr numpy Pillow google-generativeai
```

**For EasyOCR** (requires system dependencies):
```bash
pip install easyocr
```
⚠️ **IMPORTANT**: Follow [EasyOCR's GitHub instructions](https://github.com/JaidedAI/EasyOCR#installation) closely (especially windows user, please read their note).

---

Verify Installation
Run this command to check all core dependencies:

```bash
python -c "import PyQt5, Pillow, easyocr, numpy, qtawesome, google-generativeai; print('All dependencies installed successfully!')"
```
If no errors appear, you're ready to proceed.
(Note: EasyOCR requires additional system dependencies - verify its functionality by processing an image)

---

## Step 3: Run the Application

Once all dependencies are installed, you can run the application using the following command:

```bash
python main.py
```

This will launch the Manhwa OCR Tool GUI. From here, you can open a folder containing images, process OCR, and apply translations as described in the tool's documentation.

---

## Troubleshooting

Below are solutions to common issues you may encounter while using this tool. If your problem isn't listed here, consider opening an issue on the repository for further assistance.

### 1. Some Text is Not Detected
This issue typically occurs when the default settings do not align with the characteristics of your desired manhwa. While the default settings work for most manhwa, some may have unique layouts or text styles that require adjustments.

**Solution:**  
Navigate to `Settings > OCR Processing` and fine-tune the settings to better match the characteristics of your manhwa. Experiment with parameters such as text size, font style, or alignment to improve detection accuracy.

### 2. EasyOCR Does Not Work as Intended

If EasyOCR does not function correctly, the issue is often related to an incompatible or missing installation of PyTorch. EasyOCR relies on PyTorch for its core functionality, so ensuring the correct version is installed is crucial.

**Solution:**  
1. Visit the [EasyOCR GitHub Installation Guide](https://github.com/JaidedAI/EasyOCR#installation) for detailed instructions.
2. Download and install the **correct version of PyTorch** for your system by following the official [PyTorch Get Started guide](https://pytorch.org/get-started/locally/). Ensure you select the appropriate configuration (e.g., OS, Python version, CUDA support).
3. Once PyTorch is installed, retry installing EasyOCR:

   ```bash
   pip install easyocr
   ```

**Note:** If you're using a GPU, verify that your CUDA drivers are up-to-date and compatible with the installed version of PyTorch. For CPU-only setups, choose the corresponding PyTorch variant during installation.

### 3. PyQt5 Installation Issues

If PyQt5 installation fails, try installing it with the following command:

```bash
pip install PyQt5==5.15.11
```

This specifies a stable version of PyQt5.

### 4. Missing Dependencies

If you encounter any "ModuleNotFoundError" during runtime, it means a required library is missing. Use the error message to identify the missing module and install it using `pip`.

---

## Notes

- **Images Files:** Ensure that all images in the selected folder are in supported formats (e.g., PNG, JPG, JPEG).
- **Explore Settings** to finds a lot of configurable for the app.
- **OCR processing** may take some time depending on the number of images and their sizes.
- **Environment Compatibility:** Ensure your Python environment meets the minimum requirements for this tool.  
- **Logs and Debugging:** If you're still facing issues, enable debug logging (if available) to gather more information about the problem.  
- **Community Support:** For unresolved issues, feel free to open an issue on the repository. Include detailed steps to reproduce the problem, along with any relevant logs or screenshots.

---

## Contributing

Feel free to contribute to this project by submitting pull requests or opening issues in the repository.

Package distributor are needed. if you're proficient in the field please leave comment in discussion.

---

That's it! You should now have a fully functional Manhwa OCR Tool installed on your system. Happy translating!
