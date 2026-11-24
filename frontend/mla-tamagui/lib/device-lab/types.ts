export const DEVICE_LAB_SENTINEL = '__device_lab_v1__';

type LabCall = { api: string; args: unknown[]; at: number };

type LabAlert = {
  title: string;
  message?: string;
  buttons: string[];
  answered?: number;
};

export interface DeviceLab {
  version: typeof DEVICE_LAB_SENTINEL;
  /** Recorded calls to APIs that are a no-op on web: vibrate, speech, share. */
  calls: LabCall[];
  /** Every alert shown, with the index of the button that answered it. */
  alerts: LabAlert[];
}
