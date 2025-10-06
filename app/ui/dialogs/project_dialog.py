from PySide6.QtWidgets import ( QVBoxLayout, QPushButton, QProgressBar,  QLabel, QListWidget,  QLineEdit, 
                            QFileDialog, QStatusBar, QHBoxLayout, QDialog, QDialogButtonBox, QComboBox)
from PySide6.QtCore import Qt, QDir, QThread, Signal
from assets.styles import NEW_PROJECT_STYLES, WFWF_STYLES
import os, tempfile, requests
from shutil import rmtree
from bs4 import BeautifulSoup
from urllib.parse import urljoin

class ImportWFWFDialog(QDialog):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Import from WFWF")
        self.setMinimumSize(500, 300)
        
        layout = QVBoxLayout()
        self.setLayout(layout)
        
        self.url_input = QLineEdit()
        self.url_input.setPlaceholderText("Enter WFWF URL...")
        layout.addWidget(QLabel("WFWF URL:"))
        layout.addWidget(self.url_input)
        
        # Add status display area
        self.status_text = QListWidget()
        layout.addWidget(QLabel("Download Progress:"))
        layout.addWidget(self.status_text)
        
        # Add progress bar
        self.progress_bar = QProgressBar()
        self.progress_bar.setRange(0, 100)
        self.progress_bar.setValue(0)
        layout.addWidget(self.progress_bar)
        
        # Status bar
        self.status_bar = QStatusBar()
        layout.addWidget(self.status_bar)
        
        # Button layout
        button_layout = QHBoxLayout()
        self.download_btn = QPushButton("Download")
        self.download_btn.clicked.connect(self.start_download)
        self.cancel_btn = QPushButton("Cancel")
        self.cancel_btn.clicked.connect(self.reject)
        self.save_btn = QPushButton("Save Project")
        self.save_btn.setEnabled(False)
        self.save_btn.clicked.connect(self.accept)
        
        button_layout.addWidget(self.download_btn)
        button_layout.addWidget(self.cancel_btn)
        button_layout.addWidget(self.save_btn)
        layout.addLayout(button_layout)
        
        self.temp_dir = None
        self.download_worker = None
        
        self.setStyleSheet(WFWF_STYLES)
    
    def get_url(self):
        return self.url_input.text().strip()
    
    def get_temp_dir(self):
        return self.temp_dir
    
    def start_download(self):
        url = self.get_url()
        if not url:
            self.status_bar.showMessage("Please enter a valid URL", 3000)
            return
        
        self.temp_dir = tempfile.mkdtemp()
        self.download_worker = ImportDownloadWorker(url, self.temp_dir)
        self.download_worker.progress.connect(self.update_progress)
        self.download_worker.status.connect(self.update_status)
        self.download_worker.finished.connect(self.download_finished)
        
        # Disable buttons during download
        self.download_btn.setEnabled(False)
        self.url_input.setEnabled(False)
        
        # Clear previous status
        self.status_text.clear()
        self.progress_bar.setValue(0)
        
        # Start download
        self.download_worker.start()
    
    def update_progress(self, value):
        self.progress_bar.setValue(value)
    
    def update_status(self, message):
        self.status_text.addItem(message)
        self.status_text.scrollToBottom()
        self.status_bar.showMessage(message, 3000)
    
    def download_finished(self, success):
        if success:
            self.status_bar.showMessage("Download completed successfully!", 5000)
            self.save_btn.setEnabled(True)
        else:
            self.status_bar.showMessage("Download failed!", 5000)
            if self.temp_dir and os.path.exists(self.temp_dir):
                rmtree(self.temp_dir)
                self.temp_dir = None
        
        # Re-enable buttons
        self.download_btn.setEnabled(True)
        self.url_input.setEnabled(True)

class ImportDownloadWorker(QThread):
    progress = Signal(int)
    status = Signal(str)
    finished = Signal(bool)

    def __init__(self, url, output_dir):
        super().__init__()
        self.url = url
        self.output_dir = output_dir

    def run(self):
        try:
            self.status.emit(f"Fetching URL: {self.url}")
            response = requests.get(self.url)
            if response.status_code != 200:
                self.status.emit(f"Failed to fetch URL (HTTP {response.status_code})")
                self.finished.emit(False)
                return

            self.status.emit("Parsing page content...")
            soup = BeautifulSoup(response.text, 'html.parser')
            images = soup.find_all('img', class_='v-img lazyload')
            if not images:
                self.status.emit("No images found on the page!")
                self.finished.emit(False)
                return

            self.status.emit(f"Found {len(images)} images")
            os.makedirs(self.output_dir, exist_ok=True)

            valid_images = [img for img in images if 'data-original' in img.attrs]
            total_images = len(valid_images)
            
            if total_images == 0:
                self.status.emit("No valid images found with data-original attribute")
                self.finished.emit(False)
                return
                
            self.status.emit(f"Starting download of {total_images} images...")
            
            for i, img in enumerate(valid_images):
                img_url = urljoin(self.url, img['data-original'])
                img_name = os.path.basename(img_url) or f'image_{i}.jpg'
                img_path = os.path.join(self.output_dir, img_name)
                
                self.status.emit(f"Downloading {img_name}...")
                img_response = requests.get(img_url)
                if img_response.status_code == 200:
                    with open(img_path, 'wb') as f:
                        f.write(img_response.content)
                    self.status.emit(f"Downloaded {img_name}")
                else:
                    self.status.emit(f"Failed to download {img_url}")
                
                # Update progress bar
                progress_value = int((i + 1) / total_images * 100)
                self.progress.emit(progress_value)

            self.status.emit(f"Download complete! All {total_images} images saved.")
            self.finished.emit(True)
        except Exception as e:
            self.status.emit(f"Error: {str(e)}")
            self.finished.emit(False)

class NewProjectDialog(QDialog):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("New Project")
        self.setMinimumSize(600, 250)  # Increased height
        
        layout = QVBoxLayout()
        self.setLayout(layout)
        
        # Source selection
        source_layout = QHBoxLayout()
        self.edit_source = QLineEdit()
        self.edit_source.setPlaceholderText("Select an image or folder...")
        self.edit_source.setReadOnly(True)
        self.btn_choose_image = QPushButton("Image")
        self.btn_choose_image.clicked.connect(self.choose_image)
        self.btn_choose_folder = QPushButton("Folder")
        self.btn_choose_folder.clicked.connect(self.choose_folder)
        
        source_layout.addWidget(self.edit_source)
        source_layout.addWidget(self.btn_choose_image)
        source_layout.addWidget(self.btn_choose_folder)
        
        # Language selection
        language_layout = QHBoxLayout()
        self.language_combo = QComboBox()
        self.language_combo.addItems(["Korean", "Chinese", "Japanese"])
        language_layout.addWidget(QLabel("Original Language:"))
        language_layout.addWidget(self.language_combo)
        
        # Project location
        project_layout = QHBoxLayout()
        self.edit_project = QLineEdit()
        self.edit_project.setPlaceholderText("Choose project save location...")
        self.edit_project.setReadOnly(True)
        self.btn_choose_project = QPushButton("Browse")
        self.btn_choose_project.clicked.connect(self.choose_project_location)
        
        project_layout.addWidget(self.edit_project)
        project_layout.addWidget(self.btn_choose_project)
        
        # Button box
        self.button_box = QDialogButtonBox(
            QDialogButtonBox.Ok | QDialogButtonBox.Cancel,
            Qt.Horizontal, self
        )
        self.button_box.accepted.connect(self.accept)
        self.button_box.rejected.connect(self.reject)
        self.button_box.button(QDialogButtonBox.Ok).setText("Create")
        
        # Add to main layout
        layout.addWidget(QLabel("Source:"))
        layout.addLayout(source_layout)
        layout.addLayout(language_layout)  # Add language selection
        layout.addWidget(QLabel("Project Location:"))
        layout.addLayout(project_layout)
        layout.addWidget(self.button_box)
        
        # Styling
        self.setStyleSheet(NEW_PROJECT_STYLES)
    
    def choose_image(self):
        file, _ = QFileDialog.getOpenFileName(self, "Select Image", QDir.homePath(), "Images (*.png *.jpg *.jpeg)")
        if file:
            self.edit_source.setText(file)
    
    def choose_folder(self):
        folder = QFileDialog.getExistingDirectory(self, "Select Image Folder", QDir.homePath())
        if folder:
            self.edit_source.setText(folder)
    
    def choose_project_location(self):
        file, _ = QFileDialog.getSaveFileName(self, "Create Project", QDir.homePath(), "Manga Translation Files (*.mmtl)")
        if file:
            self.edit_project.setText(file)
    
    def get_paths(self):
        return self.edit_source.text(), self.edit_project.text(), self.language_combo.currentText()