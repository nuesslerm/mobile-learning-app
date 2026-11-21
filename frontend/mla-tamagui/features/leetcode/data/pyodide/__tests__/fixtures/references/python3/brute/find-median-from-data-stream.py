class MedianFinder:
    def __init__(self):
        self.values = []

    def addNum(self, num: int) -> None:
        self.values.append(num)

    def findMedian(self) -> float:
        ordered = sorted(self.values)
        n = len(ordered)
        if n % 2:
            return float(ordered[n // 2])
        return (ordered[n // 2 - 1] + ordered[n // 2]) / 2
