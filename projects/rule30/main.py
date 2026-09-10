import pygame
from pygame.locals import *

# Constants
BLACK = (0, 0, 0)
WHITE = (255, 255, 255)

SCREEN_SIZE = (1000, 1000)
CELL_SIZE = (10, 10)

RATIO = int(SCREEN_SIZE[0] / CELL_SIZE[0])

BORDER = BLACK
BACKGROUND = WHITE

cells = []
cellLocList = []


class Cell:
    def __init__(self, x, y, state=0) -> None:
        self.state = state
        self.x = x
        self.y = y

        if self.state == 0:
            pygame.draw.rect(
                screen,
                BORDER,
                pygame.Rect(x, y, CELL_SIZE[0], CELL_SIZE[1]),
                1
            )

        elif self.state == 1:
            pygame.draw.rect(
                screen,
                BLACK,
                pygame.Rect(x, y, CELL_SIZE[0], CELL_SIZE[1]),
                0
            )

        cells.append(self)
        cellLocList.append((x, y))

    def getState(self):
        return self.state

    def getLocation(self):
        return (self.x, self.y)

    def changeState(self):
        if self.state == 0:
            self.state = 1
        elif self.state == 1:
            self.state = 0
        else:
            print("Unknown state")

    def update(self, state=None):
        if state is not None:
            self.state = state

        if self.state == 0:
            pygame.draw.rect(
                screen,
                WHITE,
                pygame.Rect(self.x, self.y, CELL_SIZE[0], CELL_SIZE[1]),
                0
            )

        elif self.state == 1:
            pygame.draw.rect(
                screen,
                BLACK,
                pygame.Rect(self.x, self.y, CELL_SIZE[0], CELL_SIZE[1]),
                0
            )


def getCellByLocation(x, y):
    for cell in cells:
        if cell.getLocation() == (x, y):
            return cell
    return None

# setup

pygame.init()

screen = pygame.display.set_mode(SCREEN_SIZE)
screen.fill(WHITE)

# grid

x, y = 0, 0

for i in range(int(SCREEN_SIZE[1] / CELL_SIZE[1])):
    for j in range(int(SCREEN_SIZE[0] / CELL_SIZE[0])):
        Cell(x, y)

        x += CELL_SIZE[0]

    x = 0
    y += CELL_SIZE[1]


# r1
cell = getCellByLocation(SCREEN_SIZE[0] / 2, 0)

if cell is not None:
    cell.update(1)
else:
    print("Cell not found")


pygame.display.flip()


def cellularAutomata(cell) -> bool:
    q = cells.index(cell)

    # If this is the first cell in the row: assume the cell outside the grid is 0.
    if q % RATIO == 0:
        pstate = 0
    else:
        pstate = cells[q - 1].getState()

    # If this is the last cell in the row: assume the cell outside the grid is 0.
    if q % RATIO == RATIO - 1:
        rstate = 0
    else:
        rstate = cells[q + 1].getState()

    qstate = cell.getState()

    qnext = bool(pstate ^ (qstate or rstate))

    return qnext


# generate

def generate(gen=0):
    # Current row
    r1 = cells[gen * RATIO:(gen + 1) * RATIO]

    # Next row
    r2 = cells[(gen + 1) * RATIO:(gen + 2) * RATIO ]

    # Generate the next row
    for cell in r1:
        newState = cellularAutomata(cell)

        # Position within the row
        x = cells.index(cell) % RATIO

        r2[x].update(newState)

    # Only update the display once per generation
    pygame.display.flip()


# main loop
running = True
enterKeyPressed = False

while running:

    for event in pygame.event.get():

        if event.type == pygame.QUIT:
            running = False

        elif event.type == KEYDOWN:

            if event.key == K_RETURN:
                enterKeyPressed = True

    # Generate all remaining generations after Enter
    if enterKeyPressed:

        for i in range(RATIO - 1):
            generate(i)

        enterKeyPressed = False


pygame.quit()
