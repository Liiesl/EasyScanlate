from PySide6.QtWidgets import QProgressBar
from PySide6.QtCore import QTimer, QDateTime

class CustomProgressBar(QProgressBar):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setFixedHeight(20)
        
        # Progress animation variables
        self.current_progress = 0
        self.target_progress = 0
        self.processing_times = []
        self.start_time = None
        
        # Timers
        self.flat_progress_timer = QTimer(self)
        self.progress_timer = QTimer(self)
        self.flat_progress_timer.timeout.connect(self.update_flat_progress)
        self.progress_timer.timeout.connect(self.update_progress_smoothly)

    def start_initial_progress(self):
        """Start the initial flat progress phase"""
        self.current_progress = 0
        self.target_progress = 0
        self.processing_times.clear()
        self.start_time = QDateTime.currentDateTime()
        
        # Flat progress timer setup
        self.flat_progress_timer_duration = 5000  # 5 seconds
        self.flat_progress_timer_interval = 70
        self.flat_progress_steps = self.flat_progress_timer_duration // self.flat_progress_timer_interval
        self.flat_progress_increment = 20 / self.flat_progress_steps
        self.flat_progress_timer.start(self.flat_progress_timer_interval)

    def update_flat_progress(self):
        if self.current_progress < 20:
            self.current_progress += self.flat_progress_increment
            self.setValue(int(self.current_progress))
        else:
            self.flat_progress_timer.stop()
            self.progress_timer.start(self.calculate_dynamic_interval())

    def update_target_progress(self, progress):
        """Update target progress from external source"""
        self.target_progress = min(int(progress), 100)
        self.progress_timer.setInterval(self.calculate_dynamic_interval())

    def calculate_dynamic_interval(self):
        """Calculate interval based on remaining progress"""
        remaining = 100 - self.current_progress
        if remaining <= 0:
            return 100
        
        # Calculate based on average processing time if available
        if self.processing_times:
            avg_time = sum(self.processing_times) / len(self.processing_times)
            remaining_time = avg_time * (100 - self.current_progress)
            interval = int((remaining_time / remaining) * 1000)
            return max(50, min(interval, 500))
        
        return 100  # Default interval

    def update_progress_smoothly(self):
        remaining = self.target_progress - self.current_progress
        if remaining <= 0:
            return

        increment = max(1, min(remaining, 3))
        self.current_progress += increment
        self.setValue(int(self.current_progress))

        # Update interval dynamically
        self.progress_timer.setInterval(self.calculate_dynamic_interval())

    def record_processing_time(self):
        """Record time for one processing unit"""
        if self.start_time:
            end_time = QDateTime.currentDateTime()
            processing_time = self.start_time.msecsTo(end_time) / 1000
            self.processing_times.append(processing_time)
            self.start_time = QDateTime.currentDateTime()

    def reset(self):
        """Reset progress to zero"""
        self.current_progress = 0
        self.target_progress = 0
        self.setValue(0)
        self.processing_times.clear()
        self.flat_progress_timer.stop()
        self.progress_timer.stop()