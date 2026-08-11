export {};

declare global {
    interface Window {
        _pageArguments: {
            lp?: string,
            gbAPI?: string,
            gbAPIFilter?: (data: any) => any,
            leSearchQuery?: string,
        },
    }

    function page(name: string): Promise<void>;

    function htmlAlert(
        title: string,
        message: string,
        buttons: {
            text: string
            resolveWith?: string,
            rejectWith?: string,
            onClick?: any, // TODO Figure out what type this is supposed to be cause i genuinely have no clue
        }[],
        specialIcon?: string
    ): Promise<any>;
}
