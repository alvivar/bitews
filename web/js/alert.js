const alert = document.getElementById("alert");
const alertHeight = alert.offsetHeight;
let clearTimer = null;

function addAlert(message, clear = false, duration = 5000) {
    if (clear) {
        alert.innerHTML = "";
    }

    alert.innerHTML = `<p>${message}</p>` + alert.innerHTML;
    alert.classList.remove("hidden");

    clearTimeout(clearTimer);
    clearTimer = setTimeout(clearAlert, duration);
}

function clearAlert() {
    alert.innerHTML = "";
    alert.classList.add("hidden");
}

function updateAlertPosition(event) {
    const x = event.pageX;
    const y = event.pageY;

    alert.style.left = `${x + 8}px`;
    alert.style.top = `${y - alertHeight}px`;
}

// Main

alert.classList.add("hidden");
document.addEventListener("mousemove", updateAlertPosition);

// Exports

export { addAlert };
