# Eä Muscle: Fëanor v5.0
# Neural network weights for the Fëanor family muscle
# Architecture: 4-input → 3-hidden (ReLU) → 1-output

import numpy as np

# Input to hidden layer weights (4x3)
W1 = np.array([
    [0.123456, -0.234567, 0.345678],
    [-0.456789, 0.567890, -0.678901],
    [0.789012, -0.890123, 0.901234],
    [-0.012345, 0.123456, -0.234567]
])

# Hidden layer biases (3)
b1 = np.array([0.111111, -0.222222, 0.333333])

# Hidden to output weights (3)
W2 = np.array([0.444444, -0.555555, 0.666666])

# Output bias (1)
b2 = 0.777777

# Neural network forward pass
def forward(inputs):
    hidden = np.maximum(0, np.dot(inputs, W1) + b1)
    output = np.dot(hidden, W2) + b2
    return output

# Example usage
if __name__ == "__main__":
    test_input = np.array([1.0, -1.0, 0.5, -0.5])
    result = forward(test_input)
    print(f"Fëanor inference: {result}")
